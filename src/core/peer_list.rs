use async_std::sync::Sender;
use rckad::KadTree;
use std::collections::HashMap;
use std::iter::Iterator;
use std::net::SocketAddr;

use crate::transports::StreamMessage;

use super::peer_id::PeerId;

/// PeerList
/// use for while & Black list
#[derive(Debug, Default)]
pub(crate) struct PeerList(Vec<PeerId>, Vec<SocketAddr>);

impl PeerList {
    pub fn init(peers: Vec<PeerId>, addrs: Vec<SocketAddr>) -> Self {
        PeerList(peers, addrs)
    }

    pub fn contains_peer(&self, peer: &PeerId) -> bool {
        self.0.contains(peer)
    }

    pub fn contains_addr(&self, addr: &SocketAddr) -> bool {
        self.1.contains(addr)
    }

    pub fn add_peer(&mut self, peer: PeerId) {
        if !self.contains_peer(&peer) {
            self.0.push(peer)
        }
    }

    pub fn add_addr(&mut self, addr: SocketAddr) {
        if !self.contains_addr(&addr) {
            self.1.push(addr)
        }
    }

    pub fn remove_peer(&mut self, peer: &PeerId) -> Option<PeerId> {
        self.0.remove_item(peer)
    }

    pub fn remove_addr(&mut self, addr: &SocketAddr) -> Option<SocketAddr> {
        self.1.remove_item(addr)
    }
}

/// PeerTable
/// contains: peers(DHT) & tmp_peers(HashMap)
pub(crate) struct PeerTable {
    peers: KadTree<PeerId, Option<Sender<StreamMessage>>>,
    tmps: HashMap<PeerId, Sender<StreamMessage>>,
}

impl PeerTable {
    pub fn init(self_peer_id: PeerId) -> Self {
        PeerTable {
            peers: KadTree::new(self_peer_id, None),
            tmps: HashMap::new(),
        }
    }

    pub fn load() {}

    pub fn all(&self) -> Vec<(PeerId, &Sender<StreamMessage>)> {
        let keys = self.peers.keys();
        keys.into_iter()
            .map(|key| {
                let sender = self.get(&key).unwrap();
                (key, sender)
            })
            .collect()
    }

    /// get in DHT, DHT closest.
    pub fn get(&self, peer_id: &PeerId) -> Option<&Sender<StreamMessage>> {
        self.peers
            .search(peer_id)
            .and_then(|(_k, ref v, _is_it)| v.as_ref())
    }

    /// get in DHT (not closest).
    pub fn get_it(&self, peer_id: &PeerId) -> Option<&Sender<StreamMessage>> {
        self.peers
            .search(peer_id)
            .and_then(|(_k, v, is_it)| if is_it { v.as_ref() } else { None })
    }

    /// get in DHT, tmp_peers,  DHT closest
    pub fn get_all(&self, peer_id: &PeerId) -> Option<&Sender<StreamMessage>> {
        self.peers.search(peer_id).and_then(|(k, v, is_it)| {
            if is_it {
                v.as_ref()
            } else {
                self.tmps.get(peer_id).or(v.as_ref())
            }
        })
    }

    pub fn contains(&self, peer_id: &PeerId) -> bool {
        self.peers.contains(peer_id)
    }

    pub fn remove(&mut self, peer_id: &PeerId) -> Option<Sender<StreamMessage>> {
        let r1 = self.peers.remove(peer_id).and_then(|v| v);
        let r2 = self.remove_tmp_peer(peer_id);
        r1.or(r2)
    }

    pub fn add_tmp_peer(&mut self, peer_id: PeerId, sender: Sender<StreamMessage>) {
        self.tmps
            .entry(peer_id)
            .and_modify(|m| *m = sender.clone())
            .or_insert(sender);
    }

    pub fn remove_tmp_peer(&mut self, peer_id: &PeerId) -> Option<Sender<StreamMessage>> {
        self.tmps.remove(peer_id)
    }

    pub fn stabilize_tmp_peer(&mut self, peer_id: PeerId) {
        self.tmps
            .remove(&peer_id)
            .map(|m| self.peers.add(peer_id, Some(m)));
    }
}
