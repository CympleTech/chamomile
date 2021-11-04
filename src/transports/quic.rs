use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::{net::IpAddr, sync::Arc, time::Duration};
use structopt::StructOpt;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::{io::Result, join, select};

use crate::keys::SessionKey;

use super::{
    new_endpoint_channel, EndpointMessage, RemotePublic, TransportRecvMessage, TransportSendMessage,
};

const DOMAIN: &str = "chamomile.quic";
const SIZE_LIMIT: usize = 67108864; // 64 * 1024 * 1024 = 64 MB

/// Init and run a QuicEndpoint object.
/// You need send a socketaddr str and quic send message's addr,
/// and receiver outside message addr.
pub async fn start(
    bind_addr: SocketAddr,
    send: Sender<TransportRecvMessage>,
    recv: Receiver<TransportSendMessage>,
) -> tokio::io::Result<()> {
    let config = InternalConfig::try_from_config(Default::default()).unwrap();

    let mut builder = quinn::Endpoint::builder();
    let _ = builder.listen(config.server.clone());

    let (endpoint, mut incoming) = builder.bind(&bind_addr).unwrap();

    // QUIC listen incoming.
    let out_send = send.clone();
    tokio::spawn(async move {
        loop {
            match incoming.next().await {
                Some(quinn_conn) => match quinn_conn.await {
                    Ok(conn) => {
                        let (self_sender, self_receiver) = new_endpoint_channel();
                        let (out_sender, out_receiver) = new_endpoint_channel();

                        tokio::spawn(process_stream(
                            conn,
                            out_sender,
                            self_receiver,
                            OutType::DHT(out_send.clone(), self_sender, out_receiver),
                            None,
                        ));
                    }
                    Err(err) => {
                        error!("An incoming failed because of an error: {:?}", err);
                    }
                },
                None => {
                    break;
                }
            }
        }
    });

    // QUIC listen from outside.
    tokio::spawn(run_self_recv(endpoint, config.client, recv, send));

    Ok(())
}

async fn connect_to(
    connect: std::result::Result<quinn::Connecting, quinn::ConnectError>,
    remote_pk: RemotePublic,
) -> Result<quinn::NewConnection> {
    let conn = connect
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "connecting failure."))?
        .await?;
    let mut stream = conn.connection.open_uni().await?;
    stream
        .write_all(&EndpointMessage::Handshake(remote_pk).to_bytes())
        .await?;
    stream.finish().await?;
    Ok(conn)
}

async fn dht_connect_to(
    connect: std::result::Result<quinn::Connecting, quinn::ConnectError>,
    out_send: Sender<TransportRecvMessage>,
    remote_pk: RemotePublic,
    session_key: SessionKey,
) -> Result<()> {
    let conn = connect_to(connect, remote_pk).await?;

    let (self_sender, self_receiver) = new_endpoint_channel();
    let (out_sender, out_receiver) = new_endpoint_channel();

    process_stream(
        conn,
        out_sender,
        self_receiver,
        OutType::DHT(out_send, self_sender, out_receiver),
        Some(session_key),
    )
    .await
}

async fn stable_connect_to(
    connect: std::result::Result<quinn::Connecting, quinn::ConnectError>,
    out_sender: Sender<EndpointMessage>,
    self_receiver: Receiver<EndpointMessage>,
    remote_pk: RemotePublic,
) -> Result<()> {
    match connect_to(connect, remote_pk).await {
        Ok(conn) => process_stream(conn, out_sender, self_receiver, OutType::Stable, None).await,
        Err(_) => {
            let _ = out_sender.send(EndpointMessage::Close).await;
            Ok(())
        }
    }
}

async fn run_self_recv(
    endpoint: quinn::Endpoint,
    client_cfg: quinn::ClientConfig,
    mut recv: Receiver<TransportSendMessage>,
    out_send: Sender<TransportRecvMessage>,
) -> Result<()> {
    while let Some(m) = recv.recv().await {
        match m {
            TransportSendMessage::Connect(addr, remote_pk, session_key) => {
                let connect = endpoint.connect_with(client_cfg.clone(), &addr, DOMAIN);
                info!("QUIC dht connect to: {:?}", addr);
                tokio::spawn(dht_connect_to(
                    connect,
                    out_send.clone(),
                    remote_pk,
                    session_key,
                ));
            }
            TransportSendMessage::StableConnect(out_sender, self_receiver, addr, remote_pk) => {
                let connect = endpoint.connect_with(client_cfg.clone(), &addr, DOMAIN);
                info!("QUIC stable connect to: {:?}", addr);
                tokio::spawn(stable_connect_to(
                    connect,
                    out_sender,
                    self_receiver,
                    remote_pk,
                ));
            }
        }
    }

    Ok(())
}

enum OutType {
    DHT(
        Sender<TransportRecvMessage>,
        Sender<EndpointMessage>,
        Receiver<EndpointMessage>,
    ),
    Stable,
}

async fn process_stream(
    conn: quinn::NewConnection,
    out_sender: Sender<EndpointMessage>,
    mut self_receiver: Receiver<EndpointMessage>,
    out_type: OutType,
    has_session: Option<SessionKey>,
) -> tokio::io::Result<()> {
    let quinn::NewConnection {
        connection,
        mut uni_streams,
        ..
    } = conn;
    let addr = connection.remote_address();

    let handshake: std::result::Result<RemotePublic, ()> = select! {
        v = async {
            if let Some(result) = uni_streams.next().await {
                match result {
                    Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                        debug!("Connection terminated by peer {:?}.", addr);
                        Err(())
                    }
                    Err(err) => {
                        debug!(
                            "Failed to read incoming message on uni-stream for peer {:?} with error: {:?}",
                            addr, err
                        );
                        Err(())
                    }
                    Ok(recv) => {
                        if let Ok(bytes) = recv.read_to_end(SIZE_LIMIT).await {
                            if let Ok(EndpointMessage::Handshake(remote_pk)) =
                                EndpointMessage::from_bytes(bytes)
                            {
                                return Ok(remote_pk);
                            } else {
                                Err(())
                            }
                        } else {
                            Err(())
                        }
                    }
                }
            } else {
                Err(())
            }
        } => v,
        v = async {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            Err(())
        } => v
    };

    if handshake.is_err() {
        // close it. if is_by_self, Better send outside not connect.
        debug!("Transport: connect read publics timeout, close it.");
        return Ok(());
    }

    let remote_pk = handshake.unwrap(); // safe. checked.

    match out_type {
        OutType::Stable => {
            out_sender
                .send(EndpointMessage::Handshake(remote_pk))
                .await
                .map_err(|_e| {
                    std::io::Error::new(std::io::ErrorKind::Other, "endpoint channel missing")
                })?;
        }
        OutType::DHT(sender, self_sender, out_receiver) => {
            sender
                .send(TransportRecvMessage(
                    addr,
                    remote_pk,
                    has_session,
                    out_sender.clone(),
                    out_receiver,
                    self_sender,
                ))
                .await
                .map_err(|_e| {
                    std::io::Error::new(std::io::ErrorKind::Other, "server channel missing")
                })?;
        }
    }

    let a = async move {
        loop {
            match self_receiver.recv().await {
                Some(msg) => {
                    let mut writer = connection.open_uni().await.map_err(|_e| ())?;
                    let is_close = match msg {
                        EndpointMessage::Close => true,
                        _ => false,
                    };

                    let _ = writer.write_all(&msg.to_bytes()).await;
                    let _ = writer.finish().await;

                    if is_close {
                        break;
                    }
                }
                None => break,
            }
        }

        Err::<(), ()>(())
    };

    let b = async {
        loop {
            match uni_streams.next().await {
                Some(result) => match result {
                    Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                        debug!("Connection terminated by peer {:?}.", addr);
                        break;
                    }
                    Err(err) => {
                        debug!(
                            "Failed to read incoming message on uni-stream for peer {:?} with error: {:?}",
                            addr, err
                        );
                        break;
                    }
                    Ok(recv) => {
                        if let Ok(bytes) = recv.read_to_end(SIZE_LIMIT).await {
                            if let Ok(msg) = EndpointMessage::from_bytes(bytes) {
                                let _ = out_sender.send(msg).await;
                            }
                        }
                    }
                },
                None => break,
            }
        }

        Err::<(), ()>(())
    };

    let _ = join!(a, b);

    info!("close stream: {}", addr);
    Ok(())
}

/// Default for [`Config::idle_timeout`] (5s).
///
/// This is based on average time in which routers would close the UDP mapping to the peer if they
/// see no conversation between them.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// An error occurred when generating the TLS certificate.
    #[error("An error occurred when generating the TLS certificate")]
    CertificateGeneration(#[from] CertificateGenerationError),
}

impl From<rcgen::RcgenError> for ConfigError {
    fn from(error: rcgen::RcgenError) -> Self {
        Self::CertificateGeneration(CertificateGenerationError(error.into()))
    }
}

impl From<quinn::ParseError> for ConfigError {
    fn from(error: quinn::ParseError) -> Self {
        Self::CertificateGeneration(CertificateGenerationError(error.into()))
    }
}

impl From<rustls::TLSError> for ConfigError {
    fn from(error: rustls::TLSError) -> Self {
        Self::CertificateGeneration(CertificateGenerationError(error.into()))
    }
}

/// An error that occured when generating the TLS certificate.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct CertificateGenerationError(Box<dyn std::error::Error + Send + Sync>);

/// Quic configurations
#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq, StructOpt)]
pub struct Config {
    /// Specify if port forwarding via UPnP should be done or not. This can be set to false if the network
    /// is run locally on the network loopback or on a local area network.
    #[structopt(long)]
    pub forward_port: bool,

    /// External port number assigned to the socket address of the program.
    /// If this is provided, QP2p considers that the local port provided has been mapped to the
    /// provided external port number and automatic port forwarding will be skipped.
    #[structopt(long)]
    pub external_port: Option<u16>,

    /// External IP address of the computer on the WAN. This field is mandatory if the node is the genesis node and
    /// port forwarding is not available. In case of non-genesis nodes, the external IP address will be resolved
    /// using the Echo service.
    #[structopt(long)]
    pub external_ip: Option<IpAddr>,

    /// How long to wait to hear from a peer before timing out a connection.
    ///
    /// In the absence of any keep-alive messages, connections will be closed if they remain idle
    /// for at least this duration.
    ///
    /// If unspecified, this will default to [`DEFAULT_IDLE_TIMEOUT`].
    #[serde(default)]
    #[structopt(long, parse(try_from_str = parse_millis), value_name = "MILLIS")]
    pub idle_timeout: Option<Duration>,
}

fn parse_millis(millis: &str) -> std::result::Result<Duration, std::num::ParseIntError> {
    Ok(Duration::from_millis(millis.parse()?))
}

/// Config that has passed validation.
///
/// Generally this is a copy of [`Config`] without optional values where we would use defaults.
#[derive(Clone, Debug)]
pub(crate) struct InternalConfig {
    pub(crate) client: quinn::ClientConfig,
    pub(crate) server: quinn::ServerConfig,
    pub(crate) forward_port: bool,
    pub(crate) external_port: Option<u16>,
    pub(crate) external_ip: Option<IpAddr>,
}

impl InternalConfig {
    pub(crate) fn try_from_config(config: Config) -> Result<Self> {
        let idle_timeout = config.idle_timeout.unwrap_or(DEFAULT_IDLE_TIMEOUT);

        let mut tconfig = quinn::TransportConfig::default();
        let _ = tconfig.max_idle_timeout(Some(idle_timeout)).ok();
        let transport = Arc::new(tconfig);

        let client = Self::new_client_config(transport.clone());
        let server = Self::new_server_config(transport)?;

        Ok(Self {
            client,
            server,
            forward_port: config.forward_port,
            external_port: config.external_port,
            external_ip: config.external_ip,
        })
    }

    fn new_client_config(transport: Arc<quinn::TransportConfig>) -> quinn::ClientConfig {
        let mut config = quinn::ClientConfig {
            transport,
            ..Default::default()
        };
        Arc::make_mut(&mut config.crypto)
            .dangerous()
            .set_certificate_verifier(Arc::new(SkipCertificateVerification));
        config
    }

    fn new_server_config(transport: Arc<quinn::TransportConfig>) -> Result<quinn::ServerConfig> {
        let (cert, key) = Self::generate_cert()?;

        let mut config = quinn::ServerConfig::default();
        config.transport = transport;

        let mut config = quinn::ServerConfigBuilder::new(config);
        let _ = config.certificate(quinn::CertificateChain::from_certs(vec![cert]), key);

        Ok(config.build())
    }

    fn generate_cert() -> Result<(quinn::Certificate, quinn::PrivateKey)> {
        let cert = rcgen::generate_simple_self_signed(vec![DOMAIN.to_string()]).map_err(|_e| {
            std::io::Error::new(std::io::ErrorKind::Other, "rcgen generate failure.")
        })?;

        let cert_der = cert.serialize_der().map_err(|_e| {
            std::io::Error::new(std::io::ErrorKind::Other, "cert serialize failure.")
        })?;
        let key_der = cert.serialize_private_key_der();

        Ok((
            quinn::Certificate::from_der(&cert_der).map_err(|_e| {
                std::io::Error::new(std::io::ErrorKind::Other, "cert_cer deserialize failure.")
            })?,
            quinn::PrivateKey::from_der(&key_der).map_err(|_e| {
                std::io::Error::new(std::io::ErrorKind::Other, "cert_psk deserialize failure.")
            })?,
        ))
    }
}

struct SkipCertificateVerification;

impl rustls::ServerCertVerifier for SkipCertificateVerification {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        _presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef,
        _ocsp_response: &[u8],
    ) -> std::result::Result<rustls::ServerCertVerified, rustls::TLSError> {
        Ok(rustls::ServerCertVerified::assertion())
    }
}
