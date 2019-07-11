use actix::prelude::*;

use quinn::{
    Certificate, CertificateChain, Connection, Endpoint, IncomingStreams, PrivateKey, ServerConfig,
    ServerConfigBuilder, TransportConfig,
};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::core::peer_id::PeerID;
use crate::core::server::ServerActor;
use crate::core::session::SessionCreate;
//use crate::protocol::keys::{PrivateKey as PeerPrivateKey, PublicKey as PeerPublicKey};

use super::super::TransportType;
use super::session::{NewStream, QuicSession};

pub(crate) struct QuicListener {
    self_peer_id: PeerID,
    server_addr: Addr<ServerActor>,
    endpoint: Endpoint,
}

impl QuicListener {
    pub fn new(self_peer_id: PeerID, server_addr: Addr<ServerActor>, endpoint: Endpoint) -> Self {
        QuicListener {
            self_peer_id,
            server_addr,
            endpoint,
        }
    }
}

struct NewConnection(Connection, IncomingStreams);

impl Message for NewConnection {
    type Result = ();
}

impl Handler<NewConnection> for QuicListener {
    type Result = ();

    fn handle(&mut self, msg: NewConnection, _ctx: &mut Context<Self>) -> Self::Result {
        let (q_conn, incoming) = (msg.0, msg.1);
        let self_peer_id = self.self_peer_id.clone();
        let server_addr = self.server_addr.clone();

        QuicSession::create(|ctx| {
            ctx.add_message_stream(incoming.map_err(|_| ()).map(|stream| NewStream(stream)));
            QuicSession::new(self_peer_id, server_addr, q_conn)
        });
    }
}

impl Actor for QuicListener {
    type Context = Context<Self>;
}

impl Handler<SessionCreate> for QuicListener {
    type Result = ();

    fn handle(&mut self, msg: SessionCreate, _ctx: &mut Context<Self>) -> Self::Result {
        let socket = TransportType::extract_socket(&msg.0);
        let _ = self.endpoint.connect(&socket, "").map(|conn| {
            let _ = conn.into_0rtt().map(|(conn_driver, q_conn, incoming)| {
                tokio::runtime::current_thread::spawn(
                    conn_driver.map_err(|e| println!("Error in quinn Driver: {:?}", e)),
                );
                NewConnection(q_conn, incoming)
            });
        });
    }
}

pub(crate) fn start_quic(
    self_peer_id: PeerID,
    server_addr: Addr<ServerActor>,
    addr: SocketAddr,
) -> Recipient<SessionCreate> {
    QuicListener::create(move |ctx| {
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
        let (driver, endpoint, incoming) = endpoint_builder.bind(addr).unwrap();

        tokio::runtime::current_thread::spawn(
            driver.map_err(|e| println!("Error in quinn Driver: {:?}", e)),
        );

        ctx.add_message_stream(
            incoming
                .map_err(|_| ())
                .map(|(conn_driver, q_conn, incoming)| {
                    tokio::runtime::current_thread::spawn(
                        conn_driver.map_err(|e| println!("Error in quinn Driver: {:?}", e)),
                    );
                    NewConnection(q_conn, incoming)
                }),
        );
        QuicListener::new(self_peer_id, server_addr, endpoint)
    })
    .recipient::<SessionCreate>()
}
