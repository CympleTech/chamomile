use actix::prelude::*;
use bytes::BytesMut;
use futures::stream::Stream;
use tokio::codec::BytesCodec;

use crate::core::peer_id::PeerID;
use crate::core::server::ServerActor;
use crate::core::session::{SessionClose, SessionCreate, SessionOpen, SessionReceive, SessionSend};

pub struct ReaderStream {
    peer_id: PeerID,
    server_addr: Addr<ServerActor>,
}

impl ReaderStream {
    pub fn new(peer_id: PeerID, server_addr: Addr<ServerActor>) -> Self {
        ReaderStream {
            peer_id,
            server_addr,
        }
    }
}

impl Actor for ReaderStream {
    type Context = Context<Self>;
}

impl StreamHandler<BytesMut, std::io::Error> for ReaderStream {
    fn handle(&mut self, msg: BytesMut, ctx: &mut Context<Self>) {
        println!("value: {:?}", msg);
        self.server_addr.do_send(SessionReceive(
            self.peer_id.clone(),
            self.peer_id.clone(),
            msg.to_vec(),
        ));

        self.stopping(ctx);
    }
}
