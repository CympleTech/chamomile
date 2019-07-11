use actix::io::FramedWrite;
use actix::io::WriteHandler;
use actix::prelude::*;
use bytes::BytesMut;
use futures::stream::Stream;
use std::net::SocketAddr;
use tokio::codec::BytesCodec;
use tokio::io::AsyncWrite;
use tokio::io::WriteHalf;

use multiaddr::Multiaddr;

use crate::core::peer_id::PeerID;
use crate::protocol::keys::{PrivateKey, PublicKey};
use crate::transports::BytesMessage;

use super::server::ServerActor;

#[derive(Clone, Debug)]
pub(crate) struct SessionCreate(pub Multiaddr);

impl Message for SessionCreate {
    //type Result = Recipient<SessionSend>;
    type Result = ();
}

#[derive(Clone, Debug)]
pub(crate) struct SessionReceive(pub PeerID, pub Vec<u8>);

impl Message for SessionReceive {
    type Result = ();
}

#[derive(Clone, Debug)]
pub(crate) struct SessionSend(pub Vec<u8>, pub bool);

impl Message for SessionSend {
    type Result = ();
}

#[derive(Clone, Debug)]
pub(crate) struct SessionOpen(pub PeerID, pub SocketAddr);

impl Message for SessionOpen {
    type Result = ();
}

#[derive(Clone, Debug)]
pub(crate) struct SessionClose(pub PeerID);

impl Message for SessionClose {
    type Result = ();
}

pub(crate) struct SessionActor<T: 'static + Stream + AsyncWrite> {
    self_peer_id: PeerID,
    self_pk: PublicKey,
    self_psk: PrivateKey,
    other_peer_id: PeerID,
    other_pk: PublicKey,
    socket: SocketAddr,
    server_addr: Addr<ServerActor>,
    write_stream: FramedWrite<WriteHalf<T>, BytesCodec>,
}

impl<T: 'static + Stream + AsyncWrite> SessionActor<T> {
    pub fn new(
        self_peer_id: PeerID,
        self_pk: PublicKey,
        self_psk: PrivateKey,
        server_addr: Addr<ServerActor>,
        socket: SocketAddr,
        write_stream: FramedWrite<WriteHalf<T>, BytesCodec>,
    ) -> Self {
        Self {
            self_peer_id: self_peer_id,
            self_pk: self_pk,
            self_psk: self_psk,
            other_peer_id: Default::default(),
            other_pk: Default::default(),
            socket: socket,
            server_addr: server_addr,
            write_stream: write_stream,
        }
    }
}

impl<T: 'static + Stream + AsyncWrite> Actor for SessionActor<T> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.server_addr
            .do_send(SessionOpen(self.other_peer_id.clone(), self.socket.clone()))
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        self.server_addr
            .do_send(SessionClose(self.other_peer_id.clone()));

        Running::Stop
    }
}

impl<T: 'static + Stream + AsyncWrite> WriteHandler<std::io::Error> for SessionActor<T> {}

impl<T: 'static + Stream + AsyncWrite> StreamHandler<BytesCodec, std::io::Error>
    for SessionActor<T>
{
    fn handle(&mut self, msg: BytesCodec, _ctx: &mut Context<Self>) {}
}

impl<T: 'static + Stream + AsyncWrite> Handler<SessionSend> for SessionActor<T> {
    type Result = ();

    fn handle(&mut self, msg: SessionSend, _ctx: &mut Context<Self>) {
        let _data = msg.0;
    }
}

impl<T: 'static + Stream + AsyncWrite> Handler<SessionClose> for SessionActor<T> {
    type Result = ();

    fn handle(&mut self, msg: SessionClose, ctx: &mut Context<Self>) {
        if self.other_peer_id == msg.0 {
            self.stopping(ctx);
        }
    }
}
