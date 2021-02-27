use aes_gcm::aead::{
    generic_array::{typenum::U12, GenericArray},
    Aead, NewAead,
};
use aes_gcm::Aes256Gcm;
use ed25519_dalek::{
    Keypair as Ed25519_Keypair, PublicKey as Ed25519_PublicKey, Signature as Ed25519_Signature,
    Signer, Verifier, KEYPAIR_LENGTH, PUBLIC_KEY_LENGTH, SECRET_KEY_LENGTH, SIGNATURE_LENGTH,
};
use rand::Rng;
use std::convert::TryFrom;
use std::io::Result;
use x25519_dalek::{PublicKey as Ed25519_DH_Public, StaticSecret as Ed25519_DH_Secret};
use zeroize::Zeroize;

use chamomile_types::types::{new_io_error, PeerId};

#[derive(Copy, Clone, Debug, Zeroize)]
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
    pub fn to_byte(&self) -> u8 {
        match self {
            KeyType::Ed25519 => 1u8,
            KeyType::Lattice => 2u8,
            KeyType::None => 0u8,
        }
    }

    pub fn from_byte(i: u8) -> Result<Self> {
        match i {
            0u8 => Ok(Self::None),
            1u8 => Ok(KeyType::Ed25519),
            2u8 => Ok(KeyType::Lattice),
            _ => Err(new_io_error("key type failure.")),
        }
    }

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

    fn _dh_sk_len(&self) -> usize {
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

    fn sign(&self, keypair: &Keypair, msg: &[u8]) -> Result<Vec<u8>> {
        match self {
            KeyType::Ed25519 => {
                let mut keypair_bytes: [u8; KEYPAIR_LENGTH] = [0u8; KEYPAIR_LENGTH];
                keypair_bytes[..SECRET_KEY_LENGTH].copy_from_slice(&keypair.sk);
                keypair_bytes[SECRET_KEY_LENGTH..].copy_from_slice(&keypair.pk);
                let keypair = Ed25519_Keypair::from_bytes(&keypair_bytes)
                    .map_err(|_e| new_io_error("ed25519 sign failure."))?;
                Ok(keypair.sign(msg).to_bytes().to_vec())
            }
            _ => Ok(Default::default()),
        }
    }

    fn verify(&self, pk: &[u8], msg: &[u8], sign: &[u8]) -> Result<bool> {
        match self {
            KeyType::Ed25519 => {
                let ed_pk = Ed25519_PublicKey::from_bytes(&pk[..])
                    .map_err(|_e| new_io_error("ed25519 public from bytes failure."))?;
                Ok(ed_pk
                    .verify(
                        msg,
                        &Ed25519_Signature::try_from(&sign[..])
                            .map_err(|_e| new_io_error("ed25519 signaure from bytes failure."))?,
                    )
                    .is_ok())
            }
            _ => Ok(false),
        }
    }

    pub fn session_key(&self, self_keypair: &Keypair) -> Result<SessionKey> {
        match self {
            KeyType::Ed25519 => {
                let alice_secret = Ed25519_DH_Secret::new(&mut rand::thread_rng());
                let alice_public = Ed25519_DH_Public::from(&alice_secret).as_bytes().to_vec();

                let sign = self_keypair.sign(&alice_public[..])?;
                let random_nonce = rand::thread_rng().gen::<[u8; 12]>();
                Ok(SessionKey {
                    key: *self,
                    sk: alice_secret.to_bytes().to_vec(),
                    pk: alice_public,
                    sign: sign,
                    is_ok: false,
                    cipher: Aes256Gcm::new(GenericArray::from_slice(&[0u8; 32])),
                    nonce: random_nonce.into(),
                })
            }
            _ => Err(new_io_error("session key failure.")),
        }
    }

    fn dh(&self, sk: &[u8], pk: &[u8]) -> Result<Vec<u8>> {
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

#[derive(Default, Debug, Zeroize)]
pub struct Keypair {
    pub key: KeyType, // [u8, 1]
    pub sk: Vec<u8>,  // [u8; key.psk_len]
    pub pk: Vec<u8>,  // [u8; key.sk_len]
}

impl Keypair {
    /// only key_type and public_key.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 1 {
            return Err(new_io_error("keypair length failure."));
        }
        let key = KeyType::from_byte(bytes[0])?;
        let pk_len = key.pk_len();

        if bytes.len() != 1 + pk_len {
            return Err(new_io_error("keypair from bytes failure."));
        }
        let pk = bytes[1..].to_vec();

        return Ok(Keypair {
            key,
            pk,
            sk: vec![],
        });
    }

    /// only key_type and public_key.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![self.key.to_byte()];
        bytes.extend(&self.pk);

        bytes
    }

    // TODO add keystore
    pub fn to_db_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![self.key.to_byte()];
        bytes.extend(&self.sk);
        bytes.extend(&self.pk);

        bytes
    }

    // TODO add keystore
    pub fn from_db_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 1 {
            return Err(new_io_error("keypair from db bytes failure."));
        }
        let key = KeyType::from_byte(bytes[0])?;
        let pk_len = key.pk_len();
        let psk_len = key.psk_len();

        if bytes.len() != 1 + pk_len + psk_len {
            return Err(new_io_error("keypair from db bytes failure."));
        }
        let sk = bytes[1..(1 + psk_len)].to_vec();
        let pk = bytes[(1 + psk_len)..].to_vec();
        Ok(Self { key, sk, pk })
    }

    pub fn peer_id(&self) -> PeerId {
        let mut peer_bytes = [0u8; 32];
        peer_bytes.copy_from_slice(blake3::hash(&self.pk).as_bytes());
        PeerId(peer_bytes)
    }

    pub fn public(&self) -> Self {
        Keypair {
            key: self.key,
            sk: vec![],
            pk: self.pk.clone(),
        }
    }

    pub fn generate_session_key(&self) -> Result<SessionKey> {
        self.key.session_key(self)
    }

    pub fn complete_session_key(&self, remote: &Keypair, dh_bytes: Vec<u8>) -> Option<SessionKey> {
        if let Ok(mut session) = self.generate_session_key() {
            if session.complete(&remote.pk, dh_bytes) {
                return Some(session);
            }
        }
        None
    }

    pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>> {
        self.key
            .sign(&self, msg)
            .map_err(|_e| new_io_error("keypair sign failure."))
    }

    pub fn verify(&self, msg: &[u8], sign: &[u8]) -> bool {
        self.key.verify(&self.pk, msg, sign).unwrap_or(false)
    }

    pub fn from_pk(key: KeyType, bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() == key.pk_len() {
            Ok(Keypair {
                key,
                sk: vec![],
                pk: bytes,
            })
        } else {
            Err(new_io_error("keypair from pk failure."))
        }
    }
}

//#[derive(Zeroize)]
pub struct SessionKey {
    key: KeyType,
    sk: Vec<u8>,
    pk: Vec<u8>,
    sign: Vec<u8>,
    is_ok: bool,
    /// 256-bit key (random key from DH key)
    cipher: Aes256Gcm,
    /// 96-bit nonce (random key, when first handshake. only use this session.)
    nonce: GenericArray<u8, U12>,
}

/// Simple DH on 25519 to get AES-256 session key.
/// 1. new a tmp public_key and sign it.
/// 2. send tmp public key and signature to remote.
/// 2. receive remote tmp public_key and signature, verify it.
/// 3. use remote public_key and self tmp private key to compute.
/// 4. get session key, and encrypt / decrypt message.
impl SessionKey {
    pub fn is_ok(&self) -> bool {
        self.is_ok
    }

    pub fn complete(&mut self, remote_pk: &[u8], remote_dh: Vec<u8>) -> bool {
        if self.key.pk_len() != remote_pk.len()
            || (self.key.dh_pk_len() + self.key.sign_len()) + 12 != remote_dh.len()
        {
            return false;
        }

        let (tmp_pk, tmp_sign_nonce) = remote_dh.split_at(self.key.dh_pk_len());
        let (tmp_sign, tmp_nonce) = tmp_sign_nonce.split_at(self.key.sign_len());

        if let Ok(true) = self.key.verify(&remote_pk, tmp_pk, tmp_sign) {
            self.key
                .dh(&self.sk, tmp_pk)
                .map(|session_key| {
                    self.cipher = Aes256Gcm::new(GenericArray::from_slice(
                        blake3::hash(&session_key).as_bytes(), // [u8; 32]
                    ));
                    let mut nonce_bytes = [0u8; 12];
                    nonce_bytes.copy_from_slice(tmp_nonce);
                    self.nonce = nonce_bytes.into();
                    self.is_ok = true;
                })
                .is_ok()
        } else {
            false
        }
    }

    pub fn out_bytes(&self) -> Vec<u8> {
        let mut vec = self.pk.clone();
        vec.extend(&self.sign);
        vec.extend(self.nonce.as_slice());
        vec
    }

    pub fn encrypt(&self, msg: Vec<u8>) -> Vec<u8> {
        self.cipher
            .encrypt(&self.nonce, msg.as_ref())
            .unwrap_or(vec![])
    }

    pub fn decrypt(&self, msg: Vec<u8>) -> Result<Vec<u8>> {
        self.cipher
            .decrypt(&self.nonce, msg.as_ref())
            .map_err(|_e| new_io_error("decrypt failure."))
    }
}
