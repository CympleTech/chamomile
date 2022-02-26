use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::{sync::Arc, time::Duration};
use structopt::StructOpt;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::{io::Result, join, select, task::JoinHandle};

use crate::session_key::SessionKey;

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
    both: bool,
) -> tokio::io::Result<SocketAddr> {
    let config = InternalConfig::try_from_config(Default::default()).unwrap();

    let (endpoint, mut incoming) = quinn::Endpoint::server(config.server.clone(), bind_addr)?;
    let addr = endpoint.local_addr()?;
    info!("QUIC listening at: {:?}", addr);

    // QUIC listen incoming.
    let out_send = send.clone();
    let task = tokio::spawn(async move {
        loop {
            match incoming.next().await {
                Some(quinn_conn) => match quinn_conn.await {
                    Ok(conn) => {
                        if both {
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
    tokio::spawn(run_self_recv(endpoint, config.client, recv, send, task));

    Ok(addr)
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
    task: JoinHandle<()>,
) -> Result<()> {
    while let Some(m) = recv.recv().await {
        match m {
            TransportSendMessage::Connect(addr, remote_pk, session_key) => {
                let connect = endpoint.connect_with(client_cfg.clone(), addr, DOMAIN);
                info!("QUIC dht connect to: {:?}", addr);
                tokio::spawn(dht_connect_to(
                    connect,
                    out_send.clone(),
                    remote_pk,
                    session_key,
                ));
            }
            TransportSendMessage::StableConnect(out_sender, self_receiver, addr, remote_pk) => {
                let connect = endpoint.connect_with(client_cfg.clone(), addr, DOMAIN);
                info!("QUIC stable connect to: {:?}", addr);
                tokio::spawn(stable_connect_to(
                    connect,
                    out_sender,
                    self_receiver,
                    remote_pk,
                ));
            }
            TransportSendMessage::Stop => {
                task.abort();
                break;
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

/// Quic configurations
#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq, StructOpt)]
pub struct Config {
    /// Specify if port forwarding via UPnP should be done or not. This can be set to false if the network
    /// is run locally on the network loopback or on a local area network.
    //#[structopt(long)]
    //pub forward_port: bool,

    /// External port number assigned to the socket address of the program.
    /// If this is provided, QP2p considers that the local port provided has been mapped to the
    /// provided external port number and automatic port forwarding will be skipped.
    //#[structopt(long)]
    //pub external_port: Option<u16>,

    /// External IP address of the computer on the WAN. This field is mandatory if the node is the genesis node and
    /// port forwarding is not available. In case of non-genesis nodes, the external IP address will be resolved
    /// using the Echo service.
    //#[structopt(long)]
    //pub external_ip: Option<IpAddr>,

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
    //pub(crate) forward_port: bool,
    //pub(crate) external_port: Option<u16>,
    //pub(crate) external_ip: Option<IpAddr>,
}

impl InternalConfig {
    pub(crate) fn try_from_config(config: Config) -> Result<Self> {
        let idle_timeout =
            quinn::IdleTimeout::try_from(config.idle_timeout.unwrap_or(DEFAULT_IDLE_TIMEOUT))
                .map_err(|_e| {
                    std::io::Error::new(std::io::ErrorKind::Other, "rcgen generate failure.")
                })?;

        let mut tconfig = quinn::TransportConfig::default();
        let _ = tconfig.max_idle_timeout(Some(idle_timeout));
        let transport = Arc::new(tconfig);

        let client = Self::new_client_config(transport.clone());
        let server = Self::new_server_config(transport)?;

        Ok(Self {
            client,
            server,
            //forward_port: config.forward_port,
            //external_port: config.external_port,
            //external_ip: config.external_ip,
        })
    }

    fn new_client_config(transport: Arc<quinn::TransportConfig>) -> quinn::ClientConfig {
        let mut client_crypto = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();

        client_crypto
            .dangerous()
            .set_certificate_verifier(Arc::new(SkipCertificateVerification));

        let mut config = quinn::ClientConfig::new(Arc::new(client_crypto));
        config.transport = transport;
        config
    }

    fn new_server_config(transport: Arc<quinn::TransportConfig>) -> Result<quinn::ServerConfig> {
        let (cert, key) = Self::generate_cert()?;

        let server_crypto = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .map_err(|_e| {
                std::io::Error::new(std::io::ErrorKind::Other, "server config failure.")
            })?;
        let mut config = quinn::ServerConfig::with_crypto(Arc::new(server_crypto));
        config.transport = transport;
        Ok(config)
    }

    fn generate_cert() -> Result<(rustls::Certificate, rustls::PrivateKey)> {
        let cert = rcgen::generate_simple_self_signed(vec![DOMAIN.to_string()]).map_err(|_e| {
            std::io::Error::new(std::io::ErrorKind::Other, "rcgen generate failure.")
        })?;

        let cert_der = cert.serialize_der().map_err(|_e| {
            std::io::Error::new(std::io::ErrorKind::Other, "cert serialize failure.")
        })?;
        let key_der = cert.serialize_private_key_der();

        Ok((rustls::Certificate(cert_der), rustls::PrivateKey(key_der)))
    }
}

struct SkipCertificateVerification;

impl rustls::client::ServerCertVerifier for SkipCertificateVerification {
    fn verify_server_cert(
        &self,
        _: &rustls::Certificate,
        _: &[rustls::Certificate],
        _: &rustls::ServerName,
        _: &mut dyn Iterator<Item = &[u8]>,
        _: &[u8],
        _: std::time::SystemTime,
    ) -> std::result::Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}
