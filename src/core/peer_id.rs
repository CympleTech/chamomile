use serde_derive::{Deserialize, Serialize};

//use crate::core::primitives::PEER_ID_LENGTH;
//use bytes::Bytes;

#[derive(Clone, Default, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct PeerID(Vec<u8>); // Multihash peer id TODO Bytes or u8 array
