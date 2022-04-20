use aes_gcm::aead::{
    generic_array::{typenum::U12, GenericArray},
    Aead, NewAead,
};
use aes_gcm::Aes256Gcm;
use rand_chacha::{
    rand_core::{RngCore, SeedableRng},
    ChaChaRng,
};

use std::io::Result;
use x25519_dalek::{PublicKey as DH_Public, StaticSecret as DH_Secret};

use chamomile_types::{
    key::{Key, PublicKey},
    types::{new_io_error, PeerId},
};

//#[derive(Zeroize)]
pub struct SessionKey {
    sk: DH_Secret,
    pk: DH_Public,
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

    pub fn generate(key: &Key) -> SessionKey {
        let mut rng = ChaChaRng::from_entropy();
        let alice_secret = DH_Secret::new(&mut rng);
        let alice_public = DH_Public::from(&alice_secret);
        let sign = key.sign(alice_public.as_bytes());
        println!("Alice pk: {:?}", alice_public.as_bytes());
        let mut random_nonce = [0u8; 12];
        rng.fill_bytes(&mut random_nonce);

        SessionKey {
            sk: alice_secret,
            pk: alice_public,
            sign: sign,
            is_ok: false,
            cipher: Aes256Gcm::new(GenericArray::from_slice(&[0u8; 32])),
            nonce: random_nonce.into(),
        }
    }

    pub fn generate_complete(key: &Key, id: &PeerId, dh_bytes: Vec<u8>) -> Option<SessionKey> {
        let mut session = Self::generate(key);
        if session.complete(id, dh_bytes) {
            Some(session)
        } else {
            None
        }
    }

    pub fn complete(&mut self, id: &PeerId, remote_dh: Vec<u8>) -> bool {
        // pk_sign_len + 32 (dh_pub) + 12 (nonce)
        if remote_dh.len() < 45 {
            return false;
        }

        let (tmp_pk, tmp_nonce_sign) = remote_dh.split_at(32);
        let (tmp_nonce, pk_sign) = tmp_nonce_sign.split_at(12);
        println!("tmp_nonce: {:?}", tmp_nonce);

        if let Ok((remote_pk, sign)) = PublicKey::deserialize_pk_and_sign(pk_sign) {
            // check peer_id
            if remote_pk.peer_id() != *id {
                return false;
            }

            if remote_pk.verify(tmp_pk, &sign).is_ok() {
                let mut pk_bytes = [0u8; 32];
                println!("bob pk: {:?}", tmp_pk);
                pk_bytes.copy_from_slice(&tmp_pk);
                let bob_public: DH_Public = pk_bytes.into();
                let c_bytes = self.sk.diffie_hellman(&bob_public);
                println!("c_bytes: {:?}", c_bytes.as_bytes());
                self.cipher = Aes256Gcm::new(GenericArray::from_slice(
                    //self.sk.diffie_hellman(&bob_public).as_bytes(),
                    c_bytes.as_bytes(),
                ));
                let mut nonce_bytes = [0u8; 12];
                nonce_bytes.copy_from_slice(tmp_nonce);
                self.nonce = nonce_bytes.into();
                self.is_ok = true;

                return true;
            }
        }

        false
    }

    pub fn out_bytes(&self, pk: &PublicKey) -> Vec<u8> {
        let mut vec = self.pk.as_bytes().to_vec();
        vec.extend(&self.nonce);
        vec.extend(&pk.serialize_pk_and_sign(&self.sign));
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
