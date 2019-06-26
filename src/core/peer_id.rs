use bytes::Bytes;

#[derive(Clone, Default, Debug)]
pub struct PeerID(Bytes); // Multihash peer id
