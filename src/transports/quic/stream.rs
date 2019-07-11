use actix::prelude::*;
use bytes::BytesMut;
use futures::stream::Stream;
use tokio::codec::BytesCodec;

use crate::core::peer_id::PeerID;
use crate::core::server::ServerActor;
use crate::core::session::{SessionClose, SessionCreate, SessionOpen, SessionReceive, SessionSend};

pub struct QuicSessionStream {
    peer_id: PeerID,
    server_addr: Addr<ServerActor>,
}

impl QuicSessionStream {
    pub fn new(peer_id: PeerID, server_addr: Addr<ServerActor>) -> Self {
        QuicSessionStream {
            peer_id,
            server_addr,
        }
    }
}

impl Actor for QuicSessionStream {
    type Context = Context<Self>;
}

impl StreamHandler<BytesMut, std::io::Error> for QuicSessionStream {
    fn handle(&mut self, msg: BytesMut, _ctx: &mut Context<Self>) {
        println!("value: {:?}", msg);
        self.server_addr
            .do_send(SessionReceive(self.peer_id.clone(), msg.to_vec()))
    }
}
