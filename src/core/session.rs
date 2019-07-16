use actix::prelude::{Message, Recipient};
use multiaddr::Multiaddr;

use crate::core::peer_id::PeerID;

#[derive(Clone, Debug)]
pub(crate) struct SessionCreate(pub Multiaddr);

impl Message for SessionCreate {
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

#[derive(Clone)]
pub(crate) struct SessionOpen(pub PeerID, pub Multiaddr, pub Recipient<SessionSend>);

impl Message for SessionOpen {
    type Result = ();
}

#[derive(Clone, Debug)]
pub(crate) struct SessionClose;

impl Message for SessionClose {
    type Result = ();
}
