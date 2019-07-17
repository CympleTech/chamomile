use multiaddr::Multiaddr;
use rckad::KadTree;
use std::collections::HashMap;

use crate::core::peer_id::PeerID;

/// PeerList
/// contains: peers(DHT) & tmp_peers(HashMap)
pub struct PeerList {
    peers: KadTree<PeerID, Multiaddr>,
    tmps: HashMap<PeerID, Multiaddr>,
}

impl PeerList {
    pub(crate) fn init(self_peer_id: PeerID, self_multiaddr: Multiaddr) -> Self {
        PeerList {
            peers: KadTree::new(self_peer_id, self_multiaddr),
            tmps: HashMap::new(),
        }
    }

    pub(crate) fn load() {}

    pub fn all(&self) {}

    /// get in DHT, DHT closest.
    pub fn get(&self, peer_id: &PeerID) -> Option<&Multiaddr> {
        self.peers.search(peer_id).and_then(|(k, v, is_it)| Some(v))
    }

    /// get in DHT (not closest).
    pub fn get_it(&self, peer_id: &PeerID) -> Option<&Multiaddr> {
        self.peers
            .search(peer_id)
            .and_then(|(k, v, is_it)| if is_it { Some(v) } else { None })
    }

    /// get in DHT, tmp_peers,  DHT closest
    pub fn get_all(&self, peer_id: &PeerID) -> Option<&Multiaddr> {
        self.peers.search(peer_id).and_then(|(k, v, is_it)| {
            if is_it {
                Some(v)
            } else {
                self.tmps.get(peer_id).or(Some(v))
            }
        })
    }

    pub fn contains(&self, peer_id: &PeerID) -> bool {
        self.peers.contains(peer_id)
    }

    pub fn remove(&mut self, peer_id: &PeerID) -> Option<Multiaddr> {
        self.peers.remove(peer_id);
        None // TODO fix DHT remove add return Value
    }

    pub fn add_tmp_peer(&mut self, peer_id: PeerID, multiaddr: Multiaddr) {
        self.tmps
            .entry(peer_id)
            .and_modify(|m| *m = multiaddr.clone())
            .or_insert(multiaddr);
    }

    pub fn remove_tmp_peer(&mut self, peer_id: &PeerID) -> Option<Multiaddr> {
        self.tmps.remove(peer_id)
    }

    pub fn stabilize_tmp_peer(&mut self, peer_id: PeerID) {
        self.tmps
            .remove(&peer_id)
            .map(|m| self.peers.add(peer_id, m));
    }
}
