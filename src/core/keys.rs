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

impl PublicKey {
    pub fn peer_id(&self) -> PeerId {
        let mut sha = Sha3_256::new();
        sha.input(&self.data);
        let mut peer_bytes = [0u8; 32];
        peer_bytes.copy_from_slice(&sha.result()[..]);
        PeerId(peer_bytes)
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
}
