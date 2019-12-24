use ed25519_dalek::{
    Keypair, PublicKey as Ed25519_PublicKey, SecretKey as Ed25519_PrivateKey, PUBLIC_KEY_LENGTH,
    SECRET_KEY_LENGTH, SIGNATURE_LENGTH,
};
use serde_derive::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

use crate::core::peer_id::PeerId;

#[derive(Copy, Clone, Serialize, Deserialize)]
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

impl KeyType {
    fn pk_len(&self) -> usize {
        match self {
            KeyType::Ed25519 => PUBLIC_KEY_LENGTH,
            _ => 0,
        }
    }

    fn psk_len(&self) -> usize {
        match self {
            KeyType::Ed25519 => SECRET_KEY_LENGTH,
            _ => 0,
        }
    }

    fn sign_len(&self) -> usize {
        match self {
            KeyType::Ed25519 => SIGNATURE_LENGTH,
            _ => 0,
        }
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
    is_ok: bool,
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

    pub fn dh(&self, pk: &PublicKey) -> Result<Vec<u8>, ()> {
        match self.key_type {
            KeyType::Ed25519 => {
                // Ed25519_PrivateKey::from_bytes(&self.data[..])
                //     * Ed25519_PublicKey::from_bytes(&pk.data[..])
                Ok(vec![1u8; 32])
            }
            _ => Ok(vec![]),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ()> {
        bincode::deserialize(&bytes[..]).map_err(|_e| ())
    }
}

impl Signature {
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ()> {
        bincode::deserialize(&bytes[..]).map_err(|_e| ())
    }
}

/// Simple DH on 25519 to get AES-256 session key.
/// 1. new a tmp public_key and sign it.
/// 2. send tmp public key and signature to remote.
/// 2. receive remote tmp public_key and signature, verify it.
/// 3. use remote public_key and self tmp private key to compute.
/// 4. get session key, and encrypt / decrypt message.
impl SessionKey {
    pub fn generate(
        remote_pk: &PublicKey,
        self_pk: &PublicKey,
        self_psk: &PrivateKey,
    ) -> SessionKey {
        match self_psk.key_type {
            KeyType::Ed25519 => {
                let (mut tmp_psk, tmp_pk) = PrivateKey::generate(self_psk.key_type);
                let mut data = tmp_pk.data; // tmp_pk
                data.append(&mut self_psk.sign(&data).data); // tmp_sign
                data.append(&mut tmp_psk.data); // tmp_psk
                data.append(&mut remote_pk.data.clone()); // remote_pk
                SessionKey {
                    key_type: self_psk.key_type,
                    data: data,
                    is_ok: false,
                }
            }
            _ => Default::default(), // TODO
        }
    }

    pub fn is_ok(&self) -> bool {
        self.is_ok
    }

    pub fn in_bytes(&mut self, bytes: Vec<u8>) -> bool {
        let pk_start = self.key_type.pk_len() + self.key_type.sign_len() + self.key_type.psk_len();
        let pk_end = pk_start + self.key_type.pk_len();

        let remote_pk = PublicKey {
            key_type: self.key_type,
            data: self.data[pk_start..pk_end].to_vec(),
        };

        let tmp_pk = bytes[0..self.key_type.pk_len()].to_vec();
        let tmp_sign = bytes[self.key_type.pk_len()..].to_vec();

        if remote_pk.verify(
            &tmp_pk,
            &Signature {
                key_type: self.key_type,
                data: tmp_sign,
            },
        ) {
            let remote_tmp_pk = PublicKey {
                key_type: self.key_type,
                data: tmp_pk,
            };

            let self_tmp_psk_start = self.key_type.pk_len() + self.key_type.sign_len();
            let self_tmp_psk_end = self_tmp_psk_start + self.key_type.psk_len();
            let self_tmp_psk = PrivateKey {
                key_type: self.key_type,
                data: self.data[self_tmp_psk_start..self_tmp_psk_end].to_vec(),
            };

            self_tmp_psk
                .dh(&remote_tmp_pk)
                .map(|mut session_key| {
                    self.is_ok = true;
                    self.data.append(&mut session_key);
                })
                .is_ok()
        } else {
            false
        }
    }

    pub fn out_bytes(&self) -> Vec<u8> {
        let start = self.key_type.pk_len();
        let end = start + self.key_type.sign_len();
        self.data[start..end].to_vec()
    }

    pub fn encrypt(&self, msg: Vec<u8>) -> Vec<u8> {
        msg
    }

    pub fn decrypt(&self, msg: Vec<u8>) -> Result<Vec<u8>, ()> {
        Ok(msg)
    }
}
