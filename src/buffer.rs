use smol::channel::Sender;
use std::collections::HashMap;
use std::iter::Iterator;
use std::net::SocketAddr;

use chamomile_types::types::PeerId;

use crate::kad::KadValue;
use crate::session::SessionMessage;
use crate::transports::EndpointMessage;

pub(crate) struct Buffer {
    /// queue for connect to ip addr. if has one, not send aggin.
    dhts: Vec<SocketAddr>,
    /// queue for stable connect to peer id. if has one, add to queue buffer.
    stables: HashMap<PeerId, Vec<(u64, Vec<u8>)>>,
    /// tmp stable waiting outside to stable result. 60s if no-ok, close it.
    tmps: HashMap<PeerId, (KadValue, bool)>,
}

impl Buffer {
    pub fn init() -> Self {
        Buffer {
            dhts: vec![],
            stables: HashMap::new(),
            tmps: HashMap::new(),
        }
    }

    pub fn _add_dht(&mut self, ip: &SocketAddr) -> bool {
        if self.dhts.contains(ip) {
            false
        } else {
            self.dhts.push(*ip);
            true
        }
    }

    pub fn _remove_dht(&mut self, ip: &SocketAddr) {
        if let Some(pos) = self.dhts.iter().position(|x| x == ip) {
            self.dhts.remove(pos);
        }
    }

    pub fn contains_stable(&mut self, peer_id: &PeerId) -> bool {
        if self.stables.contains_key(peer_id) {
            true
        } else {
            self.stables.insert(*peer_id, vec![]);
            false
        }
    }

    pub fn add_stable(&mut self, peer_id: &PeerId, tid: u64, data: Vec<u8>) {
        if let Some(v) = self.stables.get_mut(peer_id) {
            v.push((tid, data));
        }
    }

    pub fn remove_stable(&mut self, peer_id: &PeerId) -> Option<Vec<(u64, Vec<u8>)>> {
        self.stables.remove(peer_id)
    }

    pub fn get_tmp_session(&self, peer_id: &PeerId) -> Option<&Sender<SessionMessage>> {
        self.tmps.get(peer_id).map(|(v, _)| &v.0)
    }

    pub fn get_tmp_endpoint(&self, peer_id: &PeerId) -> Option<&Sender<EndpointMessage>> {
        self.tmps.get(peer_id).map(|(v, _)| &v.1)
    }

    pub fn add_tmp_stable(&mut self, peer_id: PeerId, value: KadValue, is_d: bool) {
        self.tmps.insert(peer_id, (value, is_d));
    }

    pub fn remove_tmp(&mut self, peer_id: &PeerId) -> Option<(KadValue, bool)> {
        self.tmps.remove(peer_id)
    }
}
