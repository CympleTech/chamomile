use aes_gcm::aead::{generic_array::GenericArray, Aead};
use aes_gcm::{Aes256Gcm, KeyInit};
use chamomile_types::{
    key::secp256k1::{PublicKey, Secp256k1, SecretKey},
    key::{Key, Signature, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH},
    types::{new_io_error, PeerId},
};
use rand_chacha::{rand_core::SeedableRng, ChaChaRng};
use std::io::Result;

//#[derive(Zeroize)]
pub struct SessionKey {
    /// Random secret key for this session
    sk: SecretKey,
    /// The session key is success
    is_ok: bool,
    /// 256-bit key (random key from DH key)
    cipher: Aes256Gcm,
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

    pub fn generate(key: &Key) -> (SessionKey, Vec<u8>) {
        let mut rng = ChaChaRng::from_entropy();
        let sk = SecretKey::new(&mut rng);
        let pk = sk.public_key(&Secp256k1::new());
        let mut pk_bytes = pk.serialize().to_vec();
        let sign = key.sign(&pk_bytes);
        pk_bytes.extend(sign.to_bytes());

        (
            SessionKey {
                sk,
                is_ok: false,
                cipher: Aes256Gcm::new(GenericArray::from_slice(&[0u8; 32])),
            },
            pk_bytes,
        )
    }

    pub fn generate_complete(
        key: &Key,
        id: &PeerId,
        dh_bytes: Vec<u8>,
    ) -> Option<(SessionKey, Vec<u8>)> {
        let (mut session, bytes) = Self::generate(key);
        if session.complete(id, dh_bytes) {
            Some((session, bytes))
        } else {
            None
        }
    }

    pub fn complete(&mut self, id: &PeerId, remote_dh: Vec<u8>) -> bool {
        // pk_bytes (33) + sign_bytes (68)
        if remote_dh.len() != PUBLIC_KEY_LENGTH + SIGNATURE_LENGTH {
            return false;
        }

        let (tmp_pk, tmp_sign) = remote_dh.split_at(PUBLIC_KEY_LENGTH);
        match (
            PublicKey::from_slice(tmp_pk),
            Signature::from_bytes(tmp_sign),
        ) {
            (Ok(pk), Ok(sign)) => {
                if let Ok(new_id) = sign.peer_id(tmp_pk) {
                    if new_id != *id {
                        return false;
                    }
                    if let Ok(dh) = pk.mul_tweak(&Secp256k1::new(), &self.sk.into()) {
                        self.cipher =
                            Aes256Gcm::new(GenericArray::from_slice(&dh.serialize()[0..32]));
                        self.is_ok = true;
                        return true;
                    }
                }
            }
            _ => {}
        }

        false
    }

    pub fn encrypt(&self, msg: Vec<u8>) -> Vec<u8> {
        let nonce = GenericArray::from_slice(&[0u8; 12]);
        self.cipher.encrypt(&nonce, msg.as_ref()).unwrap_or(vec![])
    }

    pub fn decrypt(&self, msg: Vec<u8>) -> Result<Vec<u8>> {
        let nonce = GenericArray::from_slice(&[0u8; 12]);
        self.cipher
            .decrypt(&nonce, msg.as_ref())
            .map_err(|_e| new_io_error("decrypt failure."))
    }
}
