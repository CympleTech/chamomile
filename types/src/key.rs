use rand_core::{CryptoRng, RngCore};
use secp256k1::{
    constants::ONE,
    ecdsa::{RecoverableSignature, RecoveryId},
    Message as SecpMessage, PublicKey as SecpPublicKey, Secp256k1, SecretKey as SecpSecretKey,
};
use sha3::{Digest, Keccak256};

pub use secp256k1;

use crate::types::{new_io_error, PeerId, PEER_ID_LENGTH};

pub const SECRET_KEY_LENGTH: usize = 32;
pub const PUBLIC_KEY_LENGTH: usize = 33;
pub const SIGNATURE_LENGTH: usize = 68;

/// Public Key
#[derive(Clone)]
pub struct PublicKey(SecpPublicKey);

/// Secret Key
pub struct SecretKey(SecpSecretKey);

pub struct Signature(RecoverableSignature);

/// The keypair, include pk, sk, address
pub struct Key {
    pub pub_key: PublicKey,
    pub sec_key: SecretKey,
}

impl Key {
    pub fn from_sec_key(sec_key: SecretKey) -> Self {
        let secp = Secp256k1::new();
        let pub_key = PublicKey(sec_key.0.public_key(&secp));

        Self { pub_key, sec_key }
    }

    pub fn default() -> Self {
        let sec_key = SecretKey(SecpSecretKey::from_slice(&ONE).unwrap());
        Self::from_sec_key(sec_key)
    }

    pub fn generate<R: CryptoRng + RngCore>(rng: &mut R) -> Key {
        let sec_key = SecretKey(SecpSecretKey::new(rng));
        Self::from_sec_key(sec_key)
    }

    pub fn peer_id(&self) -> PeerId {
        self.pub_key.peer_id()
    }

    pub fn public(&self) -> PublicKey {
        self.pub_key.clone()
    }

    pub fn sign(&self, msg: &[u8]) -> Signature {
        let mut hasher = Keccak256::new();
        hasher.update(msg);
        let result = hasher.finalize();
        let msg = SecpMessage::from_slice(&result).unwrap();
        let secp = Secp256k1::new();
        let sign = secp.sign_ecdsa_recoverable(&msg, &self.sec_key.0);
        Signature(sign)
    }

    pub fn to_db_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.extend(&self.sec_key.0.secret_bytes());
        bytes
    }

    pub fn from_db_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        if bytes.len() < SECRET_KEY_LENGTH {
            return Err(new_io_error("keypair from db bytes failure."));
        }
        let sec_key = SecretKey(
            SecpSecretKey::from_slice(&bytes[..SECRET_KEY_LENGTH])
                .map_err(|_| new_io_error("secret key from db bytes failure."))?,
        );
        Ok(Self::from_sec_key(sec_key))
    }
}

impl PublicKey {
    pub fn new(pk: SecpPublicKey) -> Self {
        Self(pk)
    }

    pub fn raw(&self) -> &SecpPublicKey {
        &self.0
    }

    pub fn peer_id(&self) -> PeerId {
        let public_key = self.0.serialize_uncompressed();
        let mut hasher = Keccak256::new();
        hasher.update(&public_key[1..]);
        let result = hasher.finalize();
        let mut bytes = [0u8; PEER_ID_LENGTH];
        bytes.copy_from_slice(&result[12..]);
        PeerId(bytes)
    }
}

impl SecretKey {
    pub fn new(sk: SecpSecretKey) -> Self {
        Self(sk)
    }

    pub fn raw(&self) -> &SecpSecretKey {
        &self.0
    }
}

impl Signature {
    pub fn to_bytes(&self) -> Vec<u8> {
        let (recv, fixed) = self.0.serialize_compact();
        let mut bytes = recv.to_i32().to_le_bytes().to_vec();
        bytes.extend(&fixed);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> std::io::Result<Signature> {
        if bytes.len() != SIGNATURE_LENGTH {
            return Err(new_io_error("Invalid signature length"));
        }
        let mut fixed = [0u8; 4];
        fixed.copy_from_slice(&bytes[..4]);
        let id = i32::from_le_bytes(fixed);
        let recv = RecoveryId::from_i32(id).map_err(|_| new_io_error("Invalid signature value"))?;
        RecoverableSignature::from_compact(&bytes[4..], recv)
            .map(Signature)
            .map_err(|_| new_io_error("Invalid signature value"))
    }

    pub fn peer_id(&self, msg: &[u8]) -> std::io::Result<PeerId> {
        let mut hasher = Keccak256::new();
        hasher.update(msg);
        let result = hasher.finalize();
        let msg = SecpMessage::from_slice(&result).unwrap();

        let secp = Secp256k1::new();
        let pk = secp
            .recover_ecdsa(&msg, &self.0)
            .map_err(|_| new_io_error("Invalid signature"))?;
        Ok(PublicKey(pk).peer_id())
    }
}

impl TryFrom<&str> for PublicKey {
    type Error = std::io::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let bytes = hex::decode(s).map_err(|_| new_io_error("Invalid public key hex"))?;
        if bytes.len() != PUBLIC_KEY_LENGTH {
            return Err(new_io_error("Invalid public key length"));
        }
        Ok(PublicKey(
            SecpPublicKey::from_slice(&bytes)
                .map_err(|_| new_io_error("Invalid public key value"))?,
        ))
    }
}

impl ToString for PublicKey {
    fn to_string(&self) -> String {
        hex::encode(self.0.serialize())
    }
}

impl TryFrom<&str> for SecretKey {
    type Error = std::io::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let bytes = hex::decode(s).map_err(|_| new_io_error("Invalid secret key hex"))?;
        if bytes.len() != SECRET_KEY_LENGTH {
            return Err(new_io_error("Invalid secret key length"));
        }
        Ok(SecretKey(
            SecpSecretKey::from_slice(&bytes)
                .map_err(|_| new_io_error("Invalid secret key value"))?,
        ))
    }
}

impl ToString for SecretKey {
    fn to_string(&self) -> String {
        hex::encode(self.0.secret_bytes())
    }
}
