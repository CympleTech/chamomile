use actix::prelude::Message;
use std::net::SocketAddr;

use crate::core::peer_id::PeerID;

#[derive(Clone, Debug)]
pub struct P2PMessage(pub PeerID, pub Vec<u8>);

impl Message for P2PMessage {
    type Result = ();
}

#[derive(Clone, Debug)]
pub struct SpecialP2PMessage(pub SocketAddr, pub Vec<u8>);

impl Message for SpecialP2PMessage {
    type Result = ();
}

#[derive(Clone, Debug)]
pub struct PeerJoin(pub PeerID, pub Vec<u8>);

impl Message for PeerJoin {
    type Result = ();
}

#[derive(Clone, Debug)]
pub struct PeerJoinResult(pub PeerID, pub bool);

impl Message for PeerJoinResult {
    type Result = ();
}

#[derive(Clone, Debug)]
pub struct PeerLeave(pub PeerID);

impl Message for PeerLeave {
    type Result = ();
}
