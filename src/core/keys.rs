use ed25519_dalek::Keypair;
use serde_derive::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

use crate::core::peer_id::PeerId;

#[derive(Clone, Serialize, Deserialize)]
pub enum KeyType {
    Ed25519, // Ed25519 = 0
    Lattice, // Lattice-based = 1
    None,    // None 255
}

impl Default for KeyType {
    fn default() -> Self {
        KeyType::None
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct PublicKey {
    key_type: KeyType,
    data: Vec<u8>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct PrivateKey {
    key_type: KeyType,
    data: Vec<u8>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Signature {
    key_type: KeyType,
    data: Vec<u8>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SessionKey {
    key_type: KeyType,
    data: Vec<u8>,
}

impl PublicKey {
    pub fn peer_id(&self) -> PeerId {
        let mut sha = Sha3_256::new();
        sha.input(&self.data);
        let mut peer_bytes = [0u8; 32];
        peer_bytes.copy_from_slice(&sha.result()[..]);
        PeerId(peer_bytes)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ()> {
        bincode::deserialize(&bytes[..]).map_err(|_e| ())
    }

    pub fn verify(&self, msg: &Vec<u8>, sign: &Signature) -> bool {
        true
    }
}

impl PrivateKey {
    pub fn generate(t: KeyType) -> (PrivateKey, PublicKey) {
        match t {
            KeyType::Ed25519 => {
                let keypair = Keypair::generate(&mut rand::thread_rng());
                (
                    PrivateKey {
                        key_type: KeyType::Ed25519,
                        data: keypair.secret.as_bytes().to_vec(),
                    },
                    PublicKey {
                        key_type: KeyType::Ed25519,
                        data: keypair.public.as_bytes().to_vec(),
                    },
                )
            }
            KeyType::Lattice => (
                PrivateKey {
                    key_type: KeyType::Lattice,
                    data: vec![],
                },
                PublicKey {
                    key_type: KeyType::Lattice,
                    data: vec![],
                },
            ),
            KeyType::None => (
                PrivateKey {
                    key_type: KeyType::None,
                    data: vec![],
                },
                PublicKey {
                    key_type: KeyType::None,
                    data: vec![],
                },
            ),
        }
    }

    pub fn sign(&self, msg: &Vec<u8>) -> Signature {
        Default::default()
    }
}

impl SessionKey {
    pub fn generate(
        remote_pk: &PublicKey,
        self_pk: &PublicKey,
        self_psk: &PrivateKey,
    ) -> SessionKey {
        // TODO
        Default::default()
    }

    pub fn is_ok(&self) -> bool {
        true
    }

    pub fn in_bytes(&mut self, bytes: Vec<u8>) -> bool {
        // TODO
        true
    }

    pub fn out_bytes(&self) -> Vec<u8> {
        // TODO
        vec![]
    }

    pub fn encrypt(&self, msg: Vec<u8>) -> Vec<u8> {
        msg
    }

    pub fn decrypt(&self, msg: Vec<u8>) -> Result<Vec<u8>, ()> {
        Ok(msg)
    }
}
