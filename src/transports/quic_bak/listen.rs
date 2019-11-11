use actix::prelude::*;

use quinn::{
    Certificate, CertificateChain, ClientConfigBuilder, Connection, ConnectionDriver, Endpoint,
    IncomingStreams, PrivateKey, ServerConfig, ServerConfigBuilder, TransportConfig,
};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::core::peer_id::PeerID;
use crate::core::server::ServerActor;
use crate::core::session::SessionCreate;
//use crate::protocol::keys::{PrivateKey as PeerPrivateKey, PublicKey as PeerPublicKey};

use super::super::TransportType;
use super::session::QuicSessionActor;

pub(crate) struct QuicListenActor {
    self_peer_id: PeerID,
    server_addr: Addr<ServerActor>,
    endpoint: Endpoint,
}

impl QuicListenActor {
    pub fn new(self_peer_id: PeerID, server_addr: Addr<ServerActor>, endpoint: Endpoint) -> Self {
        QuicListenActor {
            self_peer_id,
            server_addr,
            endpoint,
        }
    }
}

struct NewConnection(ConnectionDriver, Connection, IncomingStreams);

impl Message for NewConnection {
    type Result = ();
}

impl Handler<NewConnection> for QuicListenActor {
    type Result = ();

    fn handle(&mut self, msg: NewConnection, _ctx: &mut Context<Self>) -> Self::Result {
        println!("DEBUG: new connect to quic server");
        let (driver, q_conn, incoming) = (msg.0, msg.1, msg.2);
        let self_peer_id = self.self_peer_id.clone();
        let server_addr = self.server_addr.clone();

        QuicSessionActor::new(self_peer_id, server_addr, driver, q_conn, incoming).start();
    }
}

impl Actor for QuicListenActor {
    type Context = Context<Self>;
}

impl Handler<SessionCreate> for QuicListenActor {
    type Result = ();

    fn handle(&mut self, msg: SessionCreate, ctx: &mut Context<Self>) -> Self::Result {
        let socket = TransportType::extract_socket(&msg.0);
        println!("DEBUG: create session to: {}, real: {}", msg.0, socket);
        let addr = ctx.address();

        let connect = self
            .endpoint
            .connect(&socket, "localhost")
            .unwrap()
            .and_then(move |(conn_driver, q_conn, incoming)| {
                println!("DEBUG: create session connection ok!");
                addr.do_send(NewConnection(conn_driver, q_conn, incoming));
                Ok(())
            })
            .map_err(|e| println!("ERROR: create connect failure! {}", e));

        tokio::runtime::current_thread::spawn(connect);
    }
}

pub(crate) fn start_quic(
    self_peer_id: PeerID,
    server_addr: Addr<ServerActor>,
    addr: SocketAddr,
) -> Recipient<SessionCreate> {
    QuicListenActor::create(move |ctx| {
        // config server
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert_der = cert.serialize_der().unwrap();

        let priv_key = cert.serialize_private_key_der();
        let priv_key = PrivateKey::from_der(&priv_key).unwrap();

        let server_config = ServerConfig {
            transport: Arc::new(TransportConfig {
                stream_window_uni: 0,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut cfg_builder = ServerConfigBuilder::new(server_config);
        let cert = Certificate::from_der(&cert_der).unwrap();
        cfg_builder
            .certificate(CertificateChain::from_certs(vec![cert]), priv_key)
            .unwrap();

        let (server_config, _server_cert) = (cfg_builder.build(), cert_der);
        let mut endpoint_builder = Endpoint::builder();
        endpoint_builder.listen(server_config);

        // config client
        let mut client_cfg = ClientConfigBuilder::default().build();
        let tls_cfg: &mut rustls::ClientConfig = Arc::get_mut(&mut client_cfg.crypto).unwrap();
        // this is only available when compiled with "dangerous_configuration" feature
        tls_cfg
            .dangerous()
            .set_certificate_verifier(SkipServerVerification::new());

        endpoint_builder.default_client_config(client_cfg);

        let (driver, endpoint, incoming) = endpoint_builder.bind(addr).unwrap();

        tokio::runtime::current_thread::spawn(
            driver.map_err(|e| println!("Error in Listener Driver: {:?}", e)),
        );

        ctx.add_message_stream(
            incoming
                .map_err(|_| ())
                .map(|(conn_driver, q_conn, incoming)| {
                    println!("DEBUG new connect coming...");
                    NewConnection(conn_driver, q_conn, incoming)
                }),
        );
        QuicListenActor::new(self_peer_id, server_addr, endpoint)
    })
    .recipient::<SessionCreate>()
}

struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        _presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef,
        _ocsp_response: &[u8],
    ) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
        Ok(rustls::ServerCertVerified::assertion())
    }
}
