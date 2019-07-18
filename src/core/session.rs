use actix::prelude::{Message, Recipient};
use multiaddr::Multiaddr;

use crate::core::peer_id::PeerID;

/// connect multiaddr
#[derive(Clone, Debug)]
pub(crate) struct SessionCreate(pub Multiaddr);

impl Message for SessionCreate {
    type Result = ();
}

/// from, to, data
#[derive(Clone, Debug)]
pub(crate) struct SessionReceive(pub PeerID, pub PeerID, pub Vec<u8>);

impl Message for SessionReceive {
    type Result = ();
}

/// to_peer_id, data, is_close
#[derive(Clone, Debug)]
pub(crate) struct SessionSend(pub PeerID, pub Vec<u8>, pub bool);

impl Message for SessionSend {
    type Result = ();
}

/// from, from_multiaddr, session_send_addr, join_data
#[derive(Clone)]
pub(crate) struct SessionOpen(
    pub PeerID,
    pub Multiaddr,
    pub Recipient<SessionSend>,
    pub Vec<u8>,
);

impl Message for SessionOpen {
    type Result = ();
}

/// closed remote_peer_id
#[derive(Clone, Debug)]
pub(crate) struct SessionClose(pub PeerID);

impl Message for SessionClose {
    type Result = ();
}
