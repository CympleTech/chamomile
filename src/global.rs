use smol::{channel::Sender, io::Result, lock::RwLock};
use std::sync::Arc;

use chamomile_types::{
    message::ReceiveMessage,
    types::{new_io_error, PeerId},
};

use crate::buffer::Buffer;
use crate::keys::{Keypair, SessionKey};
use crate::peer::Peer;
use crate::peer_list::PeerList;
use crate::transports::{RemotePublic, TransportSendMessage};

pub(crate) struct Global {
    pub peer: Peer,
    pub key: Keypair,
    pub transport_sender: Sender<TransportSendMessage>,
    pub out_sender: Sender<ReceiveMessage>,
    pub peer_list: Arc<RwLock<PeerList>>,
    pub buffer: Arc<RwLock<Buffer>>,
    pub is_relay_data: bool,
    pub delivery_length: usize,
}

impl Global {
    #[inline]
    pub fn peer_id(&self) -> &PeerId {
        self.peer.id()
    }

    #[inline]
    pub fn generate_remote(&self) -> (SessionKey, RemotePublic) {
        // random gennerate, so must return. no keep-loop.
        loop {
            if let Ok(session_key) = self.key.generate_session_key() {
                let remote_pk = RemotePublic(
                    self.key.public(),
                    self.peer.clone(),
                    session_key.out_bytes(),
                );
                return (session_key, remote_pk);
            }
        }
    }

    #[inline]
    pub fn complete_remote(
        &self,
        remote_key: &Keypair,
        dh_bytes: Vec<u8>,
    ) -> Option<(SessionKey, RemotePublic)> {
        if let Some(session_key) = self.key.complete_session_key(remote_key, dh_bytes) {
            let remote_pk = RemotePublic(
                self.key.public(),
                self.peer.clone(),
                session_key.out_bytes(),
            );
            Some((session_key, remote_pk))
        } else {
            None
        }
    }

    #[inline]
    pub async fn trans_send(&self, msg: TransportSendMessage) -> Result<()> {
        self.transport_sender
            .send(msg)
            .await
            .map_err(|_e| new_io_error("Transport missing"))
    }

    #[inline]
    pub async fn out_send(&self, msg: ReceiveMessage) -> Result<()> {
        self.out_sender
            .send(msg)
            .await
            .map_err(|_e| new_io_error("Outside missing"))
    }

    pub async fn tmp_to_stable(&self, peer_id: &PeerId, is_d: bool) -> Result<()> {
        let v_some = self.buffer.write().await.remove_tmp(peer_id);
        if let Some((v, is_d)) = v_some {
            if is_d {
                self.peer_list.write().await.add_stable(*peer_id, v, true);
                return Ok(());
            }
        }
        Err(new_io_error("missing buffer"))
    }

    pub async fn tmp_to_dht(&self, peer_id: &PeerId) -> Result<()> {
        let v_some = self.buffer.write().await.remove_tmp(peer_id);
        if let Some((v, is_d)) = v_some {
            if is_d {
                if self.peer_list.write().await.add_dht(v).await {
                    return Ok(());
                }
            }
        }
        Err(new_io_error("missing buffer"))
    }

    #[inline]
    pub async fn stable_to_dht(&self, peer_id: &PeerId) -> Result<()> {
        let mut buffer_lock = self.buffer.write().await;
        buffer_lock.remove_tmp(peer_id);
        buffer_lock.remove_stable(peer_id);
        drop(buffer_lock);

        self.peer_list.write().await.stable_to_dht(peer_id)
    }

    pub async fn dht_to_stable(&self, peer_id: &PeerId) -> Result<()> {
        self.peer_list.write().await.dht_to_stable(peer_id)
    }
}
