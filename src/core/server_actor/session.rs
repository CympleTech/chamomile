use actix::prelude::*;
use std::net::SocketAddr;

use crate::core::peer_id::PeerID;
use crate::protocol::keys::{PrivateKey, PublicKey};
use crate::transport::BytesMessage;

use super::server::ActorServer;

#[derive(Clone, Debug)]
pub(crate) struct SessionReceive(pub PeerID, pub Vec<u8>);

impl Message for SessionReceive {
    type Result = ();
}

#[derive(Clone, Debug)]
pub(crate) struct SessionSend(pub Vec<u8>);

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

pub(crate) struct SessionActor {
    self_peer_id: PeerID,
    self_pk: PublicKey,
    self_psk: PrivateKey,
    other_peer_id: PeerID,
    other_pk: PublicKey,
    socket: SocketAddr,
    server_addr: Addr<ActorServer>,
}

impl SessionActor {
    pub fn new(
        self_peer_id: PeerID,
        self_pk: PublicKey,
        self_psk: PrivateKey,
        server_addr: Addr<ActorServer>,
        socket: SocketAddr,
    ) -> Self {
        Self {
            self_peer_id: self_peer_id,
            self_pk: self_pk,
            self_psk: self_psk,
            other_peer_id: Default::default(),
            other_pk: Default::default(),
            socket: socket,
            server_addr: server_addr,
        }
    }
}

impl Actor for SessionActor {
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

impl StreamHandler<BytesMessage, std::io::Error> for SessionActor {
    fn handle(&mut self, msg: BytesMessage, _ctx: &mut Context<Self>) {}
}

impl Handler<SessionSend> for SessionActor {
    type Result = ();

    fn handle(&mut self, msg: SessionSend, _ctx: &mut Context<Self>) {
        let _data = msg.0;
    }
}

impl Handler<SessionClose> for SessionActor {
    type Result = ();

    fn handle(&mut self, msg: SessionClose, ctx: &mut Context<Self>) {
        if self.other_peer_id == msg.0 {
            self.stopping(ctx);
        }
    }
}
