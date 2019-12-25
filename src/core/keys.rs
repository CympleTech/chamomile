use aes_soft::{
    block_cipher_trait::generic_array::GenericArray, block_cipher_trait::BlockCipher, Aes256,
};
use ed25519_dalek::{
    Keypair, PublicKey as Ed25519_PublicKey, SecretKey as Ed25519_PrivateKey,
    Signature as Ed25519_Signature, KEYPAIR_LENGTH, PUBLIC_KEY_LENGTH, SECRET_KEY_LENGTH,
    SIGNATURE_LENGTH,
};
use serde_derive::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::ops::Rem;

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

    fn sign(
        &self,
        psk: &PrivateKey,
        msg: &Vec<u8>,
    ) -> Result<Signature, Box<dyn std::error::Error>> {
        match self {
            KeyType::Ed25519 => {
                let ed_psk = Ed25519_PrivateKey::from_bytes(&psk.data[..])?;
                let ed_pk: Ed25519_PublicKey = (&ed_psk).into();

                let mut keypair_bytes: [u8; KEYPAIR_LENGTH] = [0u8; KEYPAIR_LENGTH];
                keypair_bytes[..SECRET_KEY_LENGTH].copy_from_slice(&ed_psk.to_bytes());
                keypair_bytes[SECRET_KEY_LENGTH..].copy_from_slice(&ed_pk.to_bytes());
                let keypair = Keypair::from_bytes(&keypair_bytes).unwrap();
                Ok(Signature {
                    key_type: *self,
                    data: keypair.sign(msg).to_bytes().to_vec(),
                })
            }
            _ => Ok(Default::default()),
        }
    }

    fn verify(&self, pk: &PublicKey, msg: &Vec<u8>, sign: &Signature) -> bool {
        match self {
            KeyType::Ed25519 => {
                let ed_pk = Ed25519_PublicKey::from_bytes(&pk.data[..]).unwrap();
                ed_pk
                    .verify(msg, &Ed25519_Signature::from_bytes(&sign.data[..]).unwrap())
                    .is_ok()
            }
            _ => true,
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
        self.key_type.verify(self, msg, sign)
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

    pub fn sign(&self, msg: &Vec<u8>) -> Result<Signature, ()> {
        self.key_type.sign(self, msg).map_err(|_e| ())
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
                let mut data = tmp_pk.data; // tmp_pk 32
                data.append(&mut self_psk.sign(&data).unwrap().data); // tmp_sign 64
                data.append(&mut tmp_psk.data); // tmp_psk 32
                data.append(&mut remote_pk.data.clone()); // remote_pk 32

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
                    println!("session key is ok: {:?}", self);
                })
                .is_ok()
        } else {
            false
        }
    }

    pub fn out_bytes(&self) -> Vec<u8> {
        let end = self.key_type.pk_len() + self.key_type.sign_len();
        self.data[..end].to_vec()
    }

    pub fn key(&self) -> GenericArray<u8, <Aes256 as BlockCipher>::KeySize> {
        let mut key = [0u8; 32];
        let start = self.key_type.pk_len()
            + self.key_type.sign_len()
            + self.key_type.psk_len()
            + self.key_type.pk_len();
        key.copy_from_slice(&self.data[start..]);
        key.into()
    }

    pub fn encrypt(&self, mut msg: Vec<u8>) -> Vec<u8> {
        let key = self.key();
        let num = msg.len().rem(16);
        if num != 0 {
            msg.append(&mut vec![0; 16 - num])
        }
        block_encrypt(&key, &mut msg, 0);
        msg
    }

    pub fn decrypt(&self, mut msg: Vec<u8>) -> Result<Vec<u8>, ()> {
        let key = self.key();
        block_decrypt(&key, &mut msg, 0);

        // TODO need better fill
        let mut j = msg.len();
        for i in 1..(j + 1) {
            if msg[j - i] != 0u8 {
                j = j - i;
                break;
            }
        }

        Ok(msg[0..(j + 1)].to_vec())
    }
}

impl Debug for SessionKey {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        let start = self.key_type.pk_len() * 2 + self.key_type.sign_len() + self.key_type.psk_len();
        let mut hex = String::new();
        hex.extend(
            self.data[start..]
                .iter()
                .map(|byte| format!("{:02x?}", byte)),
        );
        write!(f, "0x{}", hex)
    }
}

fn block_encrypt(
    key: &GenericArray<u8, <Aes256 as BlockCipher>::KeySize>,
    plaintext: &mut Vec<u8>,
    len: usize,
) {
    let cipher = Aes256::new(key);

    let start = len * 16;
    let p_len = plaintext.len() - start;

    let block_len = if p_len > 16 { 16 } else { p_len };
    let mut block = GenericArray::from_mut_slice(&mut plaintext[start..(start + block_len)]);
    cipher.encrypt_block(&mut block);

    if p_len > 16 {
        block_encrypt(key, plaintext, len + 1);
    }
}

fn block_decrypt(
    key: &GenericArray<u8, <Aes256 as BlockCipher>::KeySize>,
    ciphertext: &mut [u8],
    len: usize,
) {
    let cipher = Aes256::new(key);

    let start = len * 16;
    let p_len = ciphertext.len() - start;

    let block_len = if p_len > 16 { 16 } else { p_len };
    let mut block = GenericArray::from_mut_slice(&mut ciphertext[start..(start + block_len)]);
    cipher.decrypt_block(&mut block);

    if p_len > 16 {
        block_decrypt(key, ciphertext, len + 1);
    }
}
