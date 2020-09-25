use rckad::KadTree;
use serde::{Deserialize, Serialize};
use smol::channel::Sender;
use std::collections::HashMap;
use std::iter::Iterator;
use std::net::{IpAddr, SocketAddr};

use chamomile_types::types::PeerId;

use crate::peer::Peer;
use crate::session::SessionSendMessage;

/// PeerList
/// contains: peers(DHT) & tmp_peers(HashMap)
#[derive(Deserialize, Serialize)]
pub(crate) struct PeerList {
    #[serde(skip)]
    peers: KadTree<PeerId, Option<(Sender<SessionSendMessage>, Peer)>>,
    #[serde(skip)]
    stables: HashMap<PeerId, Option<(Sender<SessionSendMessage>, Peer)>>,
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
            stables: HashMap::new(),
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
        debug!("{:?}", self.bootstraps);
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
    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or(vec![])
    }

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
        let pos = match self.whites.0.iter().position(|x| *x == *peer) {
            Some(x) => x,
            None => return None,
        };
        Some(self.whites.0.remove(pos))
    }

    pub fn remove_white_addr(&mut self, addr: &SocketAddr) -> Option<SocketAddr> {
        let pos = match self.whites.1.iter().position(|x| *x == *addr) {
            Some(x) => x,
            None => return None,
        };
        Some(self.whites.1.remove(pos))
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
        let pos = match self.blacks.0.iter().position(|x| *x == *peer) {
            Some(x) => x,
            None => return None,
        };
        Some(self.blacks.0.remove(pos))
    }

    pub fn remove_black_addr(&mut self, addr: &SocketAddr) -> Option<IpAddr> {
        let pos = match self.blacks.1.iter().position(|x| *x == addr.ip()) {
            Some(x) => x,
            None => return None,
        };
        Some(self.blacks.1.remove(pos))
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

    /// get in tmps, Stables, DHT, DHT closest.
    pub fn get(&self, peer_id: &PeerId) -> Option<&Sender<SessionSendMessage>> {
        self.tmps
            .get(peer_id)
            .or(self
                .stables
                .get(peer_id)
                .map(|g| if let Some(s) = g { Some(s) } else { None })
                .flatten()
                .or(self
                    .peers
                    .search(peer_id)
                    .and_then(|(_k, ref v, _is_it)| v.as_ref())))
            .map(|s| &s.0)
    }

    /// get in DHT (not closest).
    pub fn get_it(&self, peer_id: &PeerId) -> Option<&Sender<SessionSendMessage>> {
        self.tmps
            .get(peer_id)
            .or(self
                .stables
                .get(peer_id)
                .map(|g| if let Some(s) = g { Some(s) } else { None })
                .flatten()
                .or(self
                    .peers
                    .search(peer_id)
                    .and_then(|(_k, v, is_it)| if is_it { v.as_ref() } else { None })))
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
        self.stables.contains_key(peer_id) || self.peers.contains(peer_id)
    }

    /// Step:
    /// 1. add to boostraps;
    /// 2. add to kad.
    pub fn peer_add(
        &mut self,
        peer_id: PeerId,
        sender: Sender<SessionSendMessage>,
        peer: Peer,
    ) -> bool {
        // 1. add to boostraps.
        if !self.bootstraps.contains(peer.addr()) {
            self.add_bootstrap(peer.addr().clone());
        }

        // 2. add to kad.
        if self
            .peers
            .search(&peer_id)
            .and_then(|(_k, v, is_it)| if is_it { v.as_ref() } else { None })
            .is_none()
        {
            self.peers.add(peer_id, Some((sender, peer)));
            true
        } else {
            false
        }
    }

    /// Step:
    /// 1. remove from kad;
    /// 2. from from tmps.
    pub fn peer_remove(&mut self, peer_id: &PeerId) -> Option<(Sender<SessionSendMessage>, Peer)> {
        let r1 = self.peers.remove(peer_id).and_then(|v| v);
        let r2 = self.stable_tmp_remove(peer_id);
        r1.or(r2)
    }

    /// Disconnect Step:
    /// 1. remove from bootstrap.
    pub fn peer_disconnect(&mut self, addr: &SocketAddr) {
        let mut d: Option<usize> = None;
        for (k, i) in self.bootstraps.iter().enumerate() {
            if i == addr {
                d = Some(k);
            }
        }

        if let Some(i) = d {
            self.bootstraps.remove(i);
        }
    }

    /// Step:
    /// 1. add to tmps.
    pub fn stable_tmp_add(
        &mut self,
        peer_id: PeerId,
        sender: Sender<SessionSendMessage>,
        peer: Peer,
    ) {
        if !self.tmps.contains_key(&peer_id) {
            self.tmps.insert(peer_id, (sender, peer));
        } else {
            let _ = sender.try_send(SessionSendMessage::Close);
        }
    }

    /// Step:
    /// 1. remove from tmps.
    pub fn stable_tmp_remove(
        &mut self,
        peer_id: &PeerId,
    ) -> Option<(Sender<SessionSendMessage>, Peer)> {
        self.tmps.remove(peer_id)
    }

    /// Step:
    /// 1. remove from tmp;
    /// 2. add to bootstrap;
    /// 3. if is stable, add to stables & whitelist.
    /// 4. if not stable, add to kad.
    pub fn stable_tmp_stabilize(&mut self, peer_id: PeerId, is_stable: bool) -> bool {
        self.tmps
            .remove(&peer_id)
            .map(|m| {
                if !self.bootstraps.contains(m.1.addr()) {
                    self.add_bootstrap(m.1.addr().clone());
                }
                if is_stable {
                    self.stables.insert(peer_id, Some(m));
                    self.add_white_peer(peer_id);
                } else {
                    self.peers.add(peer_id, Some(m));
                }
            })
            .is_some()
    }

    /// PeerDisconnect Step:
    /// 1. remove from white_list;
    /// 2. remove from stables.
    pub fn stable_remove(
        &mut self,
        peer_id: &PeerId,
    ) -> Option<(Sender<SessionSendMessage>, Peer)> {
        self.remove_white_peer(peer_id);
        self.stables.remove(peer_id).flatten()
    }

    /// Peerl leave Step:
    /// 1. remove from stables.
    pub fn stable_leave(&mut self, peer_id: &PeerId) {
        self.stables.remove(peer_id);
    }

    /// PeerConnect start Step:
    /// 1. check is exsit, and add to stables;
    /// 2. add to white_list.
    pub fn pre_stable_add(&mut self, peer_id: PeerId) -> bool {
        if self.stables.contains_key(&peer_id) {
            true
        } else {
            self.stables.insert(peer_id, None);
            self.add_white_peer(peer_id);
            false
        }
    }

    /// Peer stable connect ok Step:
    /// 1. add to bootstrap;
    /// 2. add to stables;
    pub fn stable_stabilize(
        &mut self,
        peer_id: PeerId,
        sender: Sender<SessionSendMessage>,
        peer: Peer,
    ) -> bool {
        if !self.bootstraps.contains(peer.addr()) {
            self.add_bootstrap(peer.addr().clone());
        }

        if !self.stables.contains_key(&peer_id) || self.stables.get(&peer_id).unwrap().is_none() {
            self.stables.insert(peer_id, Some((sender, peer)));
            true
        } else {
            let _ = sender.try_send(SessionSendMessage::Close);
            false
        }
    }
}
