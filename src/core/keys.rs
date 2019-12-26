use aes_soft::{
    block_cipher_trait::generic_array::GenericArray, block_cipher_trait::BlockCipher, Aes256,
};
use ed25519_dalek::{
    Keypair as Ed25519_Keypair, PublicKey as Ed25519_PublicKey, Signature as Ed25519_Signature,
    KEYPAIR_LENGTH, PUBLIC_KEY_LENGTH, SECRET_KEY_LENGTH, SIGNATURE_LENGTH,
};
use serde_derive::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::ops::Rem;
use x25519_dalek::{PublicKey as Ed25519_DH_Public, StaticSecret as Ed25519_DH_Secret};

use crate::core::peer::PeerId;

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

    fn dh_sk_len(&self) -> usize {
        match self {
            KeyType::Ed25519 => 32,
            _ => 0,
        }
    }

    fn dh_pk_len(&self) -> usize {
        match self {
            KeyType::Ed25519 => 32,
            _ => 0,
        }
    }

    pub fn generate_kepair(&self) -> Keypair {
        match self {
            KeyType::Ed25519 => {
                let keypair = Ed25519_Keypair::generate(&mut rand::thread_rng());
                Keypair {
                    key: *self,
                    sk: keypair.secret.as_bytes().to_vec(),
                    pk: keypair.public.as_bytes().to_vec(),
                }
            }
            _ => Default::default(),
        }
    }

    fn sign(&self, keypair: &Keypair, msg: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        match self {
            KeyType::Ed25519 => {
                let mut keypair_bytes: [u8; KEYPAIR_LENGTH] = [0u8; KEYPAIR_LENGTH];
                keypair_bytes[..SECRET_KEY_LENGTH].copy_from_slice(&keypair.sk);
                keypair_bytes[SECRET_KEY_LENGTH..].copy_from_slice(&keypair.pk);
                let keypair = Ed25519_Keypair::from_bytes(&keypair_bytes).unwrap();
                Ok(keypair.sign(msg).to_bytes().to_vec())
            }
            _ => Ok(Default::default()),
        }
    }

    fn verify(&self, pk: &[u8], msg: &[u8], sign: &[u8]) -> bool {
        match self {
            KeyType::Ed25519 => {
                let ed_pk = Ed25519_PublicKey::from_bytes(&pk[..]).unwrap();
                ed_pk
                    .verify(msg, &Ed25519_Signature::from_bytes(&sign[..]).unwrap())
                    .is_ok()
            }
            _ => true,
        }
    }

    pub fn session_key(&self, self_keypair: &Keypair, remote_keypair: &Keypair) -> SessionKey {
        match self {
            KeyType::Ed25519 => {
                let alice_secret = Ed25519_DH_Secret::new(&mut rand::thread_rng());
                let alice_public = Ed25519_DH_Public::from(&alice_secret).as_bytes().to_vec();

                let sign = self_keypair.sign(&alice_public[..]).unwrap();
                SessionKey {
                    key: *self,
                    sk: alice_secret.to_bytes().to_vec(),
                    pk: alice_public,
                    sign: sign,
                    remote: remote_keypair.pk.clone(),
                    ss: vec![],
                }
            }
            _ => Default::default(), // TODO
        }
    }

    fn dh(&self, sk: &[u8], pk: &[u8]) -> Result<Vec<u8>, ()> {
        match self {
            KeyType::Ed25519 => {
                let mut sk_bytes = [0u8; 32];
                sk_bytes.copy_from_slice(&sk);
                let mut pk_bytes = [0u8; 32];
                pk_bytes.copy_from_slice(&pk);
                let alice_secret: Ed25519_DH_Secret = sk_bytes.into();
                let bob_public: Ed25519_DH_Public = pk_bytes.into();
                Ok(alice_secret.diffie_hellman(&bob_public).as_bytes().to_vec())
            }
            _ => Ok(vec![0u8; 32]),
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Keypair {
    pub key: KeyType,
    pub sk: Vec<u8>,
    pub pk: Vec<u8>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SessionKey {
    key: KeyType,
    sk: Vec<u8>,
    pk: Vec<u8>,
    sign: Vec<u8>,
    remote: Vec<u8>,
    ss: Vec<u8>,
}

impl Keypair {
    pub fn peer_id(&self) -> PeerId {
        let mut sha = Sha3_256::new();
        sha.input(&self.pk);
        let mut peer_bytes = [0u8; 32];
        peer_bytes.copy_from_slice(&sha.result()[..]);
        PeerId(peer_bytes)
    }

    pub fn public(&self) -> Self {
        Keypair {
            key: self.key,
            sk: vec![],
            pk: self.pk.clone(),
        }
    }

    pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, ()> {
        self.key.sign(&self, msg).map_err(|_e| ())
    }

    pub fn verify(&self, msg: &[u8], sign: &[u8]) -> bool {
        self.key.verify(&self.pk, msg, sign)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ()> {
        bincode::deserialize(&bytes[..]).map_err(|_e| ())
    }

    pub fn from_pk(key: KeyType, bytes: Vec<u8>) -> Result<Self, ()> {
        if bytes.len() == key.pk_len() {
            Ok(Keypair {
                key,
                sk: vec![],
                pk: bytes,
            })
        } else {
            Err(())
        }
    }
}

/// Simple DH on 25519 to get AES-256 session key.
/// 1. new a tmp public_key and sign it.
/// 2. send tmp public key and signature to remote.
/// 2. receive remote tmp public_key and signature, verify it.
/// 3. use remote public_key and self tmp private key to compute.
/// 4. get session key, and encrypt / decrypt message.
impl SessionKey {
    pub fn is_ok(&self) -> bool {
        !self.ss.is_empty()
    }

    pub fn in_bytes(&mut self, bytes: Vec<u8>) -> bool {
        if bytes.len() < self.key.dh_pk_len() {
            return false;
        }

        let (tmp_pk, tmp_sign) = bytes.split_at(self.key.dh_pk_len());

        if self.key.verify(&self.remote, tmp_pk, tmp_sign) {
            self.key
                .dh(&self.sk, tmp_pk)
                .map(|session_key| {
                    self.ss = session_key.to_vec();
                    println!("Debug: {:?}", self);
                })
                .is_ok()
        } else {
            false
        }
    }

    pub fn out_bytes(&self) -> Vec<u8> {
        let mut vec = self.pk.clone();
        vec.append(&mut self.sign.clone());
        vec
    }

    pub fn encrypt(&self, mut msg: Vec<u8>) -> Vec<u8> {
        // TODO append hash to it

        let num = msg.len().rem(16);
        if num != 0 {
            msg.append(&mut vec![0; 16 - num])
        }
        block_encrypt((&self.ss[..]).into(), &mut msg, 0);
        msg
    }

    pub fn decrypt(&self, mut msg: Vec<u8>) -> Result<Vec<u8>, ()> {
        block_decrypt((&self.ss[..]).into(), &mut msg, 0);

        // TODO check hash

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
        let mut hex = String::new();
        hex.extend(self.ss.iter().map(|byte| format!("{:02x?}", byte)));
        write!(f, "Shared Secret: 0x{}", hex)
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
