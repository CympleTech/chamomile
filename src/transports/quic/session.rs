use actix::prelude::*;
use tokio::codec::BytesCodec;
use tokio::codec::FramedRead;

//use multiaddr::Multiaddr;

use quinn::Connection;

use crate::core::peer_id::PeerID;
use crate::core::server::ServerActor;
use crate::core::session::{SessionClose, SessionCreate, SessionOpen, SessionReceive, SessionSend};

use super::stream::QuicSessionStream;

pub(crate) struct NewStream(pub quinn::NewStream);

impl Message for NewStream {
    type Result = ();
}

pub(crate) struct QuicSession {
    self_peer_id: PeerID,
    server_addr: Addr<ServerActor>,
    conn: Connection,
}

impl QuicSession {
    pub fn new(self_peer_id: PeerID, server_addr: Addr<ServerActor>, conn: Connection) -> Self {
        Self {
            self_peer_id,
            server_addr,
            conn,
        }
    }
}

impl Actor for QuicSession {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        //self.server_addr
        //.do_send(SessionOpen(self.other_peer_id.clone(), self.socket.clone()))
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        //self.server_addr
        //.do_send(SessionClose(self.other_peer_id.clone()));

        Running::Stop
    }
}

impl Handler<NewStream> for QuicSession {
    type Result = ();

    fn handle(&mut self, msg: NewStream, _ctx: &mut Context<Self>) -> Self::Result {
        let server_addr = self.server_addr.clone();
        let peer_id = self.self_peer_id.clone(); // TODO remote peer_id

        match msg.0 {
            quinn::NewStream::Bi(_w, r) => {
                QuicSessionStream::create(|ctx| {
                    QuicSessionStream::add_stream(FramedRead::new(r, BytesCodec::new()), ctx);
                    QuicSessionStream::new(peer_id, server_addr)
                });
            }
            quinn::NewStream::Uni(r) => {
                // Start SessionActor
                QuicSessionStream::create(|ctx| {
                    QuicSessionStream::add_stream(FramedRead::new(r, BytesCodec::new()), ctx);
                    QuicSessionStream::new(peer_id, server_addr)
                });
            }
        };
    }
}

impl Handler<SessionSend> for QuicSession {
    type Result = ();

    fn handle(&mut self, msg: SessionSend, _ctx: &mut Context<Self>) {
        let _data = msg.0;
    }
}

impl Handler<SessionClose> for QuicSession {
    type Result = ();

    fn handle(&mut self, _msg: SessionClose, ctx: &mut Context<Self>) {
        //if self.other_peer_id == msg.0 {
        self.stopping(ctx);
        //}
    }
}
