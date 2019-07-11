use actix::prelude::*;
use bytes::Bytes;
use crossbeam_channel as mpmc;
use futures::future::ok;
use quinn::{
    Certificate, CertificateChain, ClientConfig, ClientConfigBuilder, Endpoint, EndpointDriver,
    Incoming, PrivateKey, ServerConfig, ServerConfigBuilder, TransportConfig,
};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::core::session::{SessionActor, SessionCreate, SessionSend};

pub(crate) struct QuicListener {
    socket: SocketAddr,
}

impl QuicListener {
    pub fn new(socket: SocketAddr) -> Self {
        QuicListener { socket }
    }
}

impl Actor for QuicListener {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
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

        let (server_config, server_cert) = (cfg_builder.build(), cert_der);
        let mut endpoint_builder = Endpoint::builder();
        endpoint_builder.listen(server_config);
        let (driver, _endpoint, incoming) = endpoint_builder.bind(self.socket).unwrap();

        incoming.for_each(move |(conn_driver, q_conn, incoming)| {
            let peer_addr = q_conn.remote_address();
            incoming.for_each(move |stream| {
                match stream {
                    quinn::NewStream::Bi(read_stream, write_stream) => {
                        // Start SessionActor
                    }
                    quinn::NewStream::Uni(read_stream) => {
                        // Return
                    }
                };
                ok(())
            });
            ok(())
        });
    }
}

impl Handler<SessionCreate> for QuicListener {
    //type Result = Recipient<SessionSend>;
    type Result = ();

    fn handle(&mut self, msg: SessionCreate, ctx: &mut Context<Self>) -> Self::Result {
        //SessionActor.start().recipient::<SessionSend>()
    }
}
