use actix::prelude::*;

use futures::future::Future;
use futures::stream::Stream;

use bytes::BytesMut;
use std::env;
use std::net::SocketAddr;
use tokio::codec::{BytesCodec, FramedRead};

use tokio::io::AsyncRead;
//use tokio_kcp::{KcpConfig, KcpListener, KcpNoDelayConfig, KcpStream};

use crate::core::peer_id::PeerID;
use crate::core::server::ServerActor;
use crate::core::session::SessionActor;
use crate::protocol::keys::{PrivateKey, PublicKey};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum KCPMode {
    Default,
    Normal,
    Fast,
}

pub(crate) struct ListenerActor {
    self_peer_id: PeerID,
    self_pk: PublicKey,
    self_psk: PrivateKey,
    server_addr: Addr<ServerActor>,
    addr: SocketAddr,
    config: KCPMode,
}

impl ListenerActor {
    // fn get_config(mode: KCPMode) -> KcpConfig {
    //     let mut config = KcpConfig::default();
    //     config.wnd_size = Some((128, 128));
    //     match mode {
    //         KCPMode::Default => {
    //             config.nodelay = Some(KcpNoDelayConfig {
    //                 nodelay: false,
    //                 interval: 10,
    //                 resend: 0,
    //                 nc: false,
    //             });
    //         }
    //         KCPMode::Normal => {
    //             config.nodelay = Some(KcpNoDelayConfig {
    //                 nodelay: false,
    //                 interval: 10,
    //                 resend: 0,
    //                 nc: true,
    //             });
    //         }
    //         KCPMode::Fast => {
    //             config.nodelay = Some(KcpNoDelayConfig {
    //                 nodelay: true,
    //                 interval: 10,
    //                 resend: 2,
    //                 nc: true,
    //             });

    //             config.rx_minrto = Some(10);
    //             config.fast_resend = Some(1);
    //         }
    //     }

    //     config
    // }

    fn new(
        self_peer_id: PeerID,
        self_pk: PublicKey,
        self_psk: PrivateKey,
        server_addr: Addr<ServerActor>,
        addr: SocketAddr,
    ) -> Self {
        let config = KCPMode::Default;

        Self {
            self_peer_id,
            self_pk,
            self_psk,
            server_addr,
            addr,
            config,
        }
    }

    fn new_with_config(
        self_peer_id: PeerID,
        self_pk: PublicKey,
        self_psk: PrivateKey,
        server_addr: Addr<ServerActor>,
        addr: SocketAddr,
        config: KCPMode,
    ) -> Self {
        Self {
            self_peer_id,
            self_pk,
            self_psk,
            server_addr,
            addr,
            config,
        }
    }
}

impl Actor for ListenerActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // let listener = KcpListener::bind_with_config(&self.addr, &self.config).unwrap();
        // println!("listening on: {}", self.addr);

        // ctx.add_message_stream(listener.incoming().map_err(|_| ()).map(|st| {
        //     let (stream, addr) = (st.0, st.1);
        //     PeerConnect(stream, addr)
        // }));
    }
}

//#[derive(Message)]
//struct PeerConnect(pub KcpStream, pub SocketAddr);

// impl Handler<PeerConnect> for ListenerActor {
//     type Result = ();

//     fn handle(&mut self, msg: PeerConnect, _: &mut Context<Self>) {
//         let self_peer_id = self.self_peer_id.clone();
//         let self_pk = self.self_pk.clone();
//         let self_psk = self.self_psk.clone();
//         let server_addr = self.server_addr.clone();

//         SessionActor::create(move |ctx| {
//             let (r, w) = msg.0.split();
//             //SessionActor::add_stream(FramedRead::new(r, BytesCodec::new()), ctx);
//             SessionActor::new(
//                 self_peer_id,
//                 self_pk,
//                 self_psk,
//                 server_addr,
//                 msg.1,
//                 actix::io::FramedWrite::new(w, BytesCodec::new(), ctx),
//             )
//         });
//     }
// }
