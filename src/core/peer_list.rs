use async_std::sync::Sender;
use rckad::KadTree;
use std::collections::HashMap;
use std::iter::Iterator;
use std::net::SocketAddr;

use crate::transports::StreamMessage;

use super::peer::{Peer, PeerId};

/// PeerList
/// contains: peers(DHT) & tmp_peers(HashMap)
pub struct PeerList {
    peers: KadTree<PeerId, Option<(Sender<StreamMessage>, Peer)>>,
    tmps: HashMap<PeerId, (Sender<StreamMessage>, Peer)>,
    whites: (Vec<PeerId>, Vec<SocketAddr>),
    blacks: (Vec<PeerId>, Vec<SocketAddr>),
}

impl PeerList {
    pub fn init(
        self_peer_id: PeerId,
        whites: (Vec<PeerId>, Vec<SocketAddr>),
        blacks: (Vec<PeerId>, Vec<SocketAddr>),
    ) -> Self {
        PeerList {
            peers: KadTree::new(self_peer_id, None),
            tmps: HashMap::new(),
            whites: whites,
            blacks: blacks,
        }
    }
}

// Black and white list.
impl PeerList {
    pub fn bootstrap(&self) -> &Vec<SocketAddr> {
        &self.whites.1
    }

    pub fn is_white_peer(&self, peer: &PeerId) -> bool {
        self.whites.0.contains(peer)
    }

    pub fn is_white_addr(&self, addr: &SocketAddr) -> bool {
        self.whites.1.contains(addr)
    }

    pub fn add_white_peer(&mut self, peer: PeerId) {
        if !self.whites.0.contains(&peer) {
            self.whites.0.push(peer)
        }
    }

    pub fn add_white_addr(&mut self, addr: SocketAddr) {
        if !self.whites.1.contains(&addr) {
            self.whites.1.push(addr)
        }
    }

    pub fn remove_white_peer(&mut self, peer: &PeerId) -> Option<PeerId> {
        self.whites.0.remove_item(peer)
    }

    pub fn remove_white_addr(&mut self, addr: &SocketAddr) -> Option<SocketAddr> {
        self.whites.1.remove_item(addr)
    }

    pub fn is_black_peer(&self, peer: &PeerId) -> bool {
        self.blacks.0.contains(peer)
    }

    pub fn is_black_addr(&self, addr: &SocketAddr) -> bool {
        self.blacks.1.contains(addr)
    }

    pub fn add_black_peer(&mut self, peer: PeerId) {
        if !self.blacks.0.contains(&peer) {
            self.blacks.0.push(peer)
        }
    }

    pub fn add_black_addr(&mut self, addr: SocketAddr) {
        if !self.blacks.1.contains(&addr) {
            self.blacks.1.push(addr)
        }
    }

    pub fn remove_black_peer(&mut self, peer: &PeerId) -> Option<PeerId> {
        self.blacks.0.remove_item(peer)
    }

    pub fn remove_black_addr(&mut self, addr: &SocketAddr) -> Option<SocketAddr> {
        self.blacks.1.remove_item(addr)
    }
}

// DHT
impl PeerList {
    pub fn all(&self) -> Vec<(PeerId, &Sender<StreamMessage>)> {
        let keys = self.peers.keys();
        keys.into_iter()
            .map(|key| {
                let sender = self.get(&key).unwrap();
                (key, sender)
            })
            .collect()
    }

    /// get in tmps, DHT, DHT closest.
    pub fn get(&self, peer_id: &PeerId) -> Option<&Sender<StreamMessage>> {
        self.tmps
            .get(peer_id)
            .or(self
                .peers
                .search(peer_id)
                .and_then(|(_k, ref v, _is_it)| v.as_ref()))
            .map(|s| &s.0)
    }

    /// get in DHT (not closest).
    pub fn get_it(&self, peer_id: &PeerId) -> Option<&Sender<StreamMessage>> {
        self.tmps
            .get(peer_id)
            .or(self.peers.search(peer_id).and_then(
                |(_k, v, is_it)| {
                    if is_it {
                        v.as_ref()
                    } else {
                        None
                    }
                },
            ))
            .map(|s| &s.0)
    }

    /// get in DHT help
    pub fn get_dht_help(&self, peer_id: &PeerId) -> Vec<Peer> {
        // TODO closest peers

        let keys = self.peers.keys();
        keys.into_iter()
            .map(|key| {
                self.peers
                    .search(&key)
                    .and_then(|(_k, ref v, _is_it)| v.as_ref())
                    .map(|s| &s.1)
                    .unwrap()
            })
            .filter(|p| p.id() != peer_id)
            .cloned()
            .collect()
    }

    pub fn contains(&self, peer_id: &PeerId) -> bool {
        self.peers.contains(peer_id)
    }

    pub fn remove(&mut self, peer_id: &PeerId) -> Option<(Sender<StreamMessage>, Peer)> {
        let r1 = self.peers.remove(peer_id).and_then(|v| v);
        let r2 = self.remove_tmp_peer(peer_id);
        r1.or(r2)
    }

    pub fn add_tmp_peer(&mut self, peer_id: PeerId, sender: Sender<StreamMessage>, peer: Peer) {
        self.tmps
            .entry(peer_id)
            .and_modify(|m| *m = (sender.clone(), peer.clone()))
            .or_insert((sender, peer));
    }

    pub fn remove_tmp_peer(&mut self, peer_id: &PeerId) -> Option<(Sender<StreamMessage>, Peer)> {
        self.tmps.remove(peer_id)
    }

    pub fn stabilize_tmp_peer(&mut self, peer_id: PeerId) {
        self.tmps
            .remove(&peer_id)
            .map(|m| self.peers.add(peer_id, Some(m)));
    }
}
