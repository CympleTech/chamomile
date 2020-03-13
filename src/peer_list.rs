use async_std::sync::Sender;
use rckad::KadTree;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::iter::Iterator;
use std::net::{IpAddr, SocketAddr};

use crate::peer::{Peer, PeerId};
use crate::session::SessionSendMessage;
use crate::storage::LocalDB;

/// PeerList
/// contains: peers(DHT) & tmp_peers(HashMap)
#[derive(Deserialize, Serialize)]
pub(crate) struct PeerList {
    #[serde(skip)]
    peers: KadTree<PeerId, Option<(Sender<SessionSendMessage>, Peer)>>,
    #[serde(skip)]
    tmps: HashMap<PeerId, (Sender<SessionSendMessage>, Peer)>,
    #[serde(skip)]
    whites: (Vec<PeerId>, Vec<SocketAddr>),
    #[serde(skip)]
    blacks: (Vec<PeerId>, Vec<IpAddr>),
    bootstraps: Vec<SocketAddr>,
}

impl PeerList {
    pub fn init(
        self_peer_id: PeerId,
        whites: (Vec<PeerId>, Vec<SocketAddr>),
        blacks: (Vec<PeerId>, Vec<IpAddr>),
    ) -> Self {
        PeerList {
            peers: KadTree::new(self_peer_id, None),
            tmps: HashMap::new(),
            bootstraps: whites.1.clone(),
            whites: whites,
            blacks: blacks,
        }
    }

    pub fn merge(
        &mut self,
        peer_id: PeerId,
        whites: (Vec<PeerId>, Vec<SocketAddr>),
        blacks: (Vec<PeerId>, Vec<IpAddr>),
    ) {
        println!("{:?}", self.bootstraps);
        for addr in &whites.1 {
            self.add_bootstrap(*addr);
        }

        self.peers = KadTree::new(peer_id, None);
        self.whites = whites;
        self.blacks = blacks;
    }
}

// Black and white list.
impl PeerList {
    pub fn bootstrap(&self) -> &Vec<SocketAddr> {
        &self.bootstraps
    }

    pub fn add_bootstrap(&mut self, addr: SocketAddr) {
        if !self.bootstraps.contains(&addr) {
            self.bootstraps.push(addr)
        }
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
        self.blacks.1.contains(&addr.ip())
    }

    pub fn add_black_peer(&mut self, peer: PeerId) {
        if !self.blacks.0.contains(&peer) {
            self.blacks.0.push(peer)
        }
    }

    pub fn add_black_addr(&mut self, addr: SocketAddr) {
        if !self.blacks.1.contains(&addr.ip()) {
            self.blacks.1.push(addr.ip())
        }
    }

    pub fn remove_black_peer(&mut self, peer: &PeerId) -> Option<PeerId> {
        self.blacks.0.remove_item(peer)
    }

    pub fn remove_black_addr(&mut self, addr: &SocketAddr) -> Option<IpAddr> {
        self.blacks.1.remove_item(&addr.ip())
    }
}

// DHT
impl PeerList {
    pub fn all(&self) -> Vec<(PeerId, &Sender<SessionSendMessage>)> {
        let keys = self.peers.keys();
        keys.into_iter()
            .map(|key| {
                let sender = self.get(&key).unwrap();
                (key, sender)
            })
            .collect()
    }

    /// get in tmps, DHT, DHT closest.
    pub fn get(&self, peer_id: &PeerId) -> Option<&Sender<SessionSendMessage>> {
        self.tmps
            .get(peer_id)
            .or(self
                .peers
                .search(peer_id)
                .and_then(|(_k, ref v, _is_it)| v.as_ref()))
            .map(|s| &s.0)
    }

    /// get in DHT (not closest).
    pub fn get_it(&self, peer_id: &PeerId) -> Option<&Sender<SessionSendMessage>> {
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

    pub fn remove(&mut self, peer_id: &PeerId) -> Option<(Sender<SessionSendMessage>, Peer)> {
        let r1 = self.peers.remove(peer_id).and_then(|v| v);
        let r2 = self.remove_tmp_peer(peer_id);
        r1.or(r2)
    }

    pub fn add_tmp_peer(
        &mut self,
        peer_id: PeerId,
        sender: Sender<SessionSendMessage>,
        peer: Peer,
    ) {
        self.tmps
            .entry(peer_id)
            .and_modify(|m| *m = (sender.clone(), peer.clone()))
            .or_insert((sender, peer));
    }

    pub fn remove_tmp_peer(
        &mut self,
        peer_id: &PeerId,
    ) -> Option<(Sender<SessionSendMessage>, Peer)> {
        self.tmps.remove(peer_id)
    }

    pub fn stabilize_tmp_peer(&mut self, peer_id: PeerId, key: &Vec<u8>, db: &mut LocalDB) {
        self.tmps.remove(&peer_id).map(|m| {
            if !self.bootstraps.contains(m.1.addr()) {
                self.add_bootstrap(m.1.addr().clone());
                let _ = db.update(key.clone(), self);
            }
            self.peers.add(peer_id, Some(m));
        });
    }
}
