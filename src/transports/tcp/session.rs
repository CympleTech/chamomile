use actix::io::FramedWrite;
use actix::io::WriteHandler;
use actix::prelude::*;
use bytes::BytesMut;
use futures::stream::Stream;
use std::net::SocketAddr;
use tokio::codec::BytesCodec;
use tokio::io::AsyncWrite;
use tokio::io::WriteHalf;
use tokio::net::tcp::TcpStream;

use multiaddr::Multiaddr;

use crate::core::peer_id::PeerID;
use crate::core::server::ServerActor;
use crate::core::session::{SessionClose, SessionCreate, SessionOpen, SessionReceive, SessionSend};
use crate::protocol::keys::{PrivateKey, PublicKey};

use super::super::TransportType;

pub struct TcpSessionActor {
    self_peer_id: PeerID,
    server_addr: Addr<ServerActor>,
    remote_multiaddr: Multiaddr,
    framed: FramedWrite<WriteHalf<TcpStream>, BytesCodec>,
}

impl Actor for TcpSessionActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.server_addr.do_send(SessionOpen(
            self.self_peer_id.clone(),
            self.remote_multiaddr.clone(),
            ctx.address().recipient::<SessionSend>(),
        ));
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        self.server_addr.do_send(SessionClose);
        Running::Stop
    }
}

impl WriteHandler<std::io::Error> for TcpSessionActor {}

impl StreamHandler<BytesMut, std::io::Error> for TcpSessionActor {
    fn handle(&mut self, msg: BytesMut, _ctx: &mut Self::Context) {
        println!("DEBUG: SessionActor received data: {:?}", msg);
    }
}

impl Handler<SessionSend> for TcpSessionActor {
    type Result = ();

    fn handle(&mut self, msg: SessionSend, _ctx: &mut Context<Self>) {
        println!("DEBUG: SessionActor send data: {:?}", msg.0);

        self.framed.write(msg.0.into());
    }
}

impl Handler<SessionClose> for TcpSessionActor {
    type Result = ();

    fn handle(&mut self, _msg: SessionClose, ctx: &mut Context<Self>) {
        self.stopping(ctx);
    }
}

impl TcpSessionActor {
    pub fn new(
        self_peer_id: PeerID,
        server_addr: Addr<ServerActor>,
        framed: FramedWrite<WriteHalf<TcpStream>, BytesCodec>,
        remote_socket: SocketAddr,
    ) -> TcpSessionActor {
        let remote_multiaddr = TransportType::TCP.to_multiaddr(&remote_socket);

        TcpSessionActor {
            self_peer_id,
            server_addr,
            framed,
            remote_multiaddr,
        }
    }

    pub fn heartbeat(&self, ctx: &mut Context<Self>) {}
}
