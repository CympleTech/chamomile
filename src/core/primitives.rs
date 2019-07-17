use multiaddr::Multiaddr;
use serde_derive::{Deserialize, Serialize};

use crate::core::peer_id::PeerID;
use crate::protocol::keys::PublicKey;

pub const PEER_ID_LENGTH: usize = 42;

#[derive(Serialize, Deserialize)]
pub enum DataType {
    Identity(String, PublicKey), //String -> Multiaddr
    DHT(Vec<(PeerID, String)>),  // String -> Multiaddr
    DH(Vec<u8>),
    RawData(Vec<u8>),
    Hole(String), // String -> Multiaddr
    Ping,
    Pong,
}
