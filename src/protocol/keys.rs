use bytes::Bytes;

use crate::core::peer_id::PeerID;

pub enum KeyType {
    RSA,       //RSA = 0,
    Ed25519,   //Ed25519 = 1;
    Secp256k1, //Secp256k1 = 2;
    ECDSA,     //ECDSA = 3;
}

pub struct PublicKey {
    key_type: KeyType,
    data: Bytes,
}

pub struct PrivateKey {
    key_type: KeyType,
    data: Bytes,
}

impl PublicKey {
    pub fn peer_id(&self) -> PeerID {
        Default::default()
    }
}
