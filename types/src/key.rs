use ed25519_dalek::{
    Keypair as Ed25519_Keypair, PublicKey as Ed25519_PublicKey, Signature as Ed25519_Signature,
    Signer, Verifier, KEYPAIR_LENGTH, PUBLIC_KEY_LENGTH,
};
use rand_core::{CryptoRng, RngCore};
use ripemd::{Digest, Ripemd160};
use serde::{Deserialize, Serialize};
use sha3::Sha3_256;

use crate::types::{new_io_error, PeerId};

pub enum Key {
    Ed25519(Ed25519_Keypair),
}

impl Key {
    pub fn default() -> Self {
        Key::Ed25519(Ed25519_Keypair::from_bytes(&[0u8; KEYPAIR_LENGTH]).unwrap())
    }

    pub fn generate<R: CryptoRng + RngCore>(rng: &mut R) -> Key {
        let keypair = Ed25519_Keypair::generate(rng);
        Key::Ed25519(keypair)
    }

    pub fn peer_id(&self) -> PeerId {
        self.public().peer_id()
    }

    pub fn public(&self) -> PublicKey {
        match self {
            Key::Ed25519(key) => PublicKey::Ed25519(key.public),
        }
    }

    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        match self {
            Key::Ed25519(key) => key.sign(msg).to_bytes().to_vec(),
        }
    }

    pub fn to_db_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![];
        match self {
            Key::Ed25519(key) => {
                bytes.push(0);
                bytes.extend_from_slice(&key.to_bytes());
            }
        }
        bytes
    }

    pub fn from_db_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        if bytes.len() < 1 {
            return Err(new_io_error("keypair from db bytes failure."));
        }
        match bytes[0] {
            0u8 => {
                let key = Ed25519_Keypair::from_bytes(&bytes[1..])
                    .map_err(|_e| new_io_error("keypair from db bytes failure."))?;
                Ok(Key::Ed25519(key))
            }
            _ => Err(new_io_error("keypair from db bytes failure.")),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum PublicKey {
    Ed25519(Ed25519_PublicKey),
}

impl PublicKey {
    pub fn peer_id(&self) -> PeerId {
        let mut peer_bytes = [0u8; 20];
        let mut hash1 = Sha3_256::new();
        let mut hash2 = Ripemd160::new();

        match self {
            PublicKey::Ed25519(pk) => hash1.update(pk.as_bytes()),
        }

        hash2.update(&hash1.finalize()[..]);
        peer_bytes.copy_from_slice(&hash2.finalize()[..]);
        PeerId(peer_bytes)
    }

    pub fn serialize_pk_and_sign(&self, sign: &[u8]) -> Vec<u8> {
        let mut bytes = vec![];
        match self {
            PublicKey::Ed25519(pk) => {
                bytes.push(0);
                bytes.extend_from_slice(pk.as_bytes());
                bytes.extend_from_slice(sign)
            }
        }
        bytes
    }

    pub fn deserialize_pk_and_sign(bytes: &[u8]) -> std::io::Result<(PublicKey, Vec<u8>)> {
        match bytes[0] {
            0u8 => {
                if bytes.len() < PUBLIC_KEY_LENGTH + 1 {
                    return Err(new_io_error("deserialize public key failure."));
                }

                let pk = Ed25519_PublicKey::from_bytes(&bytes[1..PUBLIC_KEY_LENGTH + 1])
                    .map_err(|_| new_io_error("deserialize public key failure."))?;
                Ok((
                    PublicKey::Ed25519(pk),
                    bytes[PUBLIC_KEY_LENGTH + 1..].to_vec(),
                ))
            }
            _ => Err(new_io_error("deserialize public key failure.")),
        }
    }

    pub fn verify(&self, msg: &[u8], sign: &[u8]) -> std::io::Result<()> {
        match self {
            PublicKey::Ed25519(pk) => {
                let s = Ed25519_Signature::try_from(&sign[..])
                    .map_err(|_e| new_io_error("ed25519 signaure from bytes failure."))?;

                pk.verify(msg, &s)
                    .map_err(|_e| new_io_error("ed25519 verify failure."))
            }
        }
    }
}
