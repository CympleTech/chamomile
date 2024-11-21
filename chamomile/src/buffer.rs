use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::mpsc::Sender;

use chamomile_types::{Peer, PeerId};

use crate::kad::KadValue;
use crate::session::SessionMessage;
use crate::transports::EndpointMessage;

#[derive(Hash, Eq, PartialEq, Clone)]
pub enum BufferKey {
    Peer(PeerId),
    Addr(SocketAddr),
}

pub(crate) struct Buffer {
    /// queue for connect to ip addr. if has one, not send aggin.
    dhts: HashMap<SocketAddr, bool>,
    /// queue for stable connect to peer id. if has one, add to queue buffer.
    connects: HashMap<BufferKey, (bool, Vec<(u64, Vec<u8>)>)>,
    /// queue for stable result to peer id. if has one, add to queue buffer.
    results: HashMap<PeerId, (bool, Vec<(u64, Vec<u8>)>)>,
    /// tmp stable waiting outside to stable result. 60s if no-ok, close it.
    tmps: HashMap<PeerId, (bool, KadValue, bool)>,
}

impl Buffer {
    pub fn init() -> Self {
        Buffer {
            dhts: HashMap::new(),
            connects: HashMap::new(),
            results: HashMap::new(),
            tmps: HashMap::new(),
        }
    }

    pub fn _add_dht(&mut self, ip: &SocketAddr) -> bool {
        if self.dhts.contains_key(ip) {
            false
        } else {
            self.dhts.insert(*ip, false);
            true
        }
    }

    pub fn _remove_dht(&mut self, ip: &SocketAddr) {
        self.dhts.remove(ip);
    }

    /// Result is already had or none.
    pub fn add_connect(&mut self, key: BufferKey, tid: u64, data: Vec<u8>) -> bool {
        if let Some(v) = self.connects.get_mut(&key) {
            v.1.push((tid, data));
            true
        } else {
            self.connects.insert(key, (false, vec![(tid, data)]));
            false
        }
    }

    pub fn remove_connect(&mut self, key: BufferKey) -> Vec<(u64, Vec<u8>)> {
        self.connects.remove(&key).map(|v| v.1).unwrap_or(vec![])
    }

    pub fn add_result(&mut self, peer_id: PeerId, tid: u64, data: Vec<u8>) -> bool {
        if let Some(v) = self.results.get_mut(&peer_id) {
            v.1.push((tid, data));
            true
        } else {
            self.results.insert(peer_id, (false, vec![(tid, data)]));
            false
        }
    }

    pub fn remove_result(&mut self, peer_id: &PeerId) -> Vec<(u64, Vec<u8>)> {
        self.results.remove(peer_id).map(|v| v.1).unwrap_or(vec![])
    }

    pub fn remove_stable(&mut self, peer_id: &PeerId) {
        self.connects.remove(&BufferKey::Peer(*peer_id));
        self.results.remove(peer_id);
    }

    pub fn get_tmp_session(&self, peer_id: &PeerId) -> Option<&Sender<SessionMessage>> {
        self.tmps.get(peer_id).map(|(_, v, _)| &v.0)
    }

    pub fn get_tmp_stream(&self, peer_id: &PeerId) -> Option<&Sender<EndpointMessage>> {
        self.tmps.get(peer_id).map(|(_, v, _)| &v.1)
    }

    pub fn add_tmp(&mut self, peer_id: PeerId, value: KadValue, is_d: bool) {
        self.tmps.insert(peer_id, (false, value, is_d));
    }

    pub fn update_peer(&mut self, peer_id: &PeerId, peer: Peer) {
        self.tmps.get_mut(peer_id).map(|(_, v, _)| v.2 = peer);
    }

    pub fn remove_tmp(&mut self, peer_id: &PeerId) -> Option<(KadValue, bool)> {
        self.tmps.remove(peer_id).map(|(_, v, is_d)| (v, is_d))
    }

    pub async fn timer_clear(&mut self) {
        let mut dht_deletes = vec![];
        for (ip, t) in self.dhts.iter_mut() {
            if *t {
                dht_deletes.push(*ip);
            } else {
                *t = true; // checked.
            }
        }
        for ip in dht_deletes {
            self.dhts.remove(&ip);
        }

        let mut connect_deletes = vec![];
        for (id, (t, _)) in self.connects.iter_mut() {
            if *t {
                connect_deletes.push(id.clone());
            } else {
                *t = true; // checked.
            }
        }
        for id in connect_deletes {
            self.connects.remove(&id);
        }

        let mut result_deletes = vec![];
        for (id, (t, _)) in self.results.iter_mut() {
            if *t {
                result_deletes.push(*id);
            } else {
                *t = true; // checked.
            }
        }
        for id in result_deletes {
            self.results.remove(&id);
        }

        let mut tmp_deletes = vec![];
        for (id, (t, KadValue(ss, _, _), _)) in self.tmps.iter_mut() {
            if *t {
                let _ = ss.send(SessionMessage::Close).await;
                tmp_deletes.push(*id);
            } else {
                *t = true; // checked.
            }
        }
        for id in tmp_deletes {
            self.tmps.remove(&id);
        }
    }
}
