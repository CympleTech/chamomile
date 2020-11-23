use smol::{channel::Sender, fs};
use std::collections::HashMap;
use std::io::BufRead;
use std::iter::Iterator;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use chamomile_types::types::PeerId;

use crate::kad::{KadTree, KadValue};
use crate::peer::Peer;
use crate::session::SessionMessage;
use crate::transports::EndpointMessage;

/// PeerList
/// contains: dhts(DHT) & tmp_dhts(HashMap)
pub(crate) struct PeerList {
    save_path: PathBuf,
    /// PeerId => KadValue(Sender<Sessionmessage>, Sender<EndpointMessage>, Peer)
    dhts: KadTree,
    /// PeerId => KadValue(Sender<SessionMessage>, Sender<EndpointMessage>, Peer)
    stables: HashMap<PeerId, (KadValue, bool)>,
    tmp_stables: HashMap<PeerId, (Sender<SessionMessage>, Sender<EndpointMessage>)>,
    whites: (Vec<PeerId>, Vec<SocketAddr>),
    blacks: (Vec<PeerId>, Vec<IpAddr>),
    bootstraps: Vec<SocketAddr>,
}

impl PeerList {
    pub async fn save(&self) {
        let mut file_string = String::new();
        for addr in &self.bootstraps {
            file_string = format!("{}\n{}", file_string, addr.to_string());
        }
        let _ = fs::write(&self.save_path, file_string).await;
    }

    pub fn load(
        peer_id: PeerId,
        save_path: PathBuf,
        whites: (Vec<PeerId>, Vec<SocketAddr>),
        blacks: (Vec<PeerId>, Vec<IpAddr>),
    ) -> Self {
        let mut bootstraps = whites.1.clone();
        match std::fs::File::open(&save_path) {
            Ok(file) => {
                let addrs = std::io::BufReader::new(file).lines();
                for addr in addrs {
                    if let Ok(addr) = addr {
                        if let Ok(socket) = addr.parse::<SocketAddr>() {
                            if !bootstraps.contains(&socket) {
                                bootstraps.push(socket);
                            }
                        }
                    }
                }
                PeerList {
                    save_path,
                    bootstraps,
                    dhts: KadTree::new(peer_id),
                    stables: HashMap::new(),
                    tmp_stables: HashMap::new(),
                    whites: whites,
                    blacks: blacks,
                }
            }
            Err(_) => PeerList {
                save_path,
                bootstraps,
                dhts: KadTree::new(peer_id),
                stables: HashMap::new(),
                tmp_stables: HashMap::new(),
                whites: whites,
                blacks: blacks,
            },
        }
    }

    /// get all peers in the peer list.
    pub fn all(&self) -> HashMap<PeerId, &Sender<SessionMessage>> {
        let mut peers: HashMap<PeerId, &Sender<SessionMessage>> = HashMap::new();
        for key in self.dhts.keys().into_iter() {
            if let Some((sender, _, _)) = self.dht_get(&key) {
                peers.insert(key, sender);
            }
        }

        for (p, v) in self.stables.iter() {
            peers.insert(*p, &(v.0).0);
        }

        peers
    }

    pub fn dht_keys(&self) -> Vec<PeerId> {
        self.dhts.keys()
    }

    /// get all stable peers in the peer list.
    pub fn stable_all(&self) -> HashMap<PeerId, (&Sender<SessionMessage>, bool)> {
        self.stables
            .iter()
            .map(|(k, v)| (*k, (&(v.0).0, v.1)))
            .collect()
    }

    /// search in stable list and DHT table. result is channel sender and if is it.
    pub fn get(
        &self,
        peer_id: &PeerId,
    ) -> Option<(&Sender<SessionMessage>, &Sender<EndpointMessage>, bool)> {
        self.stable_get(peer_id).or(self.dht_get(peer_id))
    }

    /// search in dht table.
    pub fn dht_get(
        &self,
        peer_id: &PeerId,
    ) -> Option<(&Sender<SessionMessage>, &Sender<EndpointMessage>, bool)> {
        self.dhts
            .search(peer_id)
            .map(|(_k, v, is_it)| (&v.0, &v.1, is_it))
    }

    /// search in stable list.
    pub fn stable_get(
        &self,
        peer_id: &PeerId,
    ) -> Option<(&Sender<SessionMessage>, &Sender<EndpointMessage>, bool)> {
        self.stables
            .get(peer_id)
            .map(|v| (&(v.0).0, &(v.0).1, true))
    }

    /// if peer has connected in peer list.
    pub fn contains(&self, peer_id: &PeerId) -> bool {
        self.stables.contains_key(peer_id) || self.dhts.contains(peer_id)
    }

    /// if peer has stable connected in peer list.
    pub fn stable_contains(&self, peer_id: &PeerId) -> bool {
        self.stables.contains_key(peer_id)
    }

    pub fn stable_relay_contains(&self, peer_id: &PeerId) -> Option<&Sender<SessionMessage>> {
        self.stables
            .get(peer_id)
            .map(|v| if v.1 { Some(&(v.0).0) } else { None })
            .flatten()
    }

    /// get in DHT help
    pub fn get_dht_help(&self, peer_id: &PeerId) -> Vec<Peer> {
        // TODO better closest peers

        let mut peers: HashMap<&PeerId, &Peer> = HashMap::new();
        for key in self.dhts.keys().into_iter() {
            if &key == peer_id {
                continue;
            }
            if let Some((k, KadValue(_, _, peer), is_it)) = self.dhts.search(&key) {
                if is_it {
                    peers.insert(k, peer);
                }
            }
        }

        for (p, v) in self.stables.iter() {
            if p != peer_id {
                peers.insert(p, &(v.0).2);
            }
        }

        peers.values().map(|v| *v.clone()).collect()
    }

    /// Step:
    /// 1. add to boostraps;
    /// 2. add to kad.
    pub async fn peer_add(
        &mut self,
        peer_id: PeerId,
        sender: Sender<SessionMessage>,
        stream_sender: Sender<EndpointMessage>,
        peer: Peer,
    ) -> bool {
        // 1. add to boostraps.
        if !self.bootstraps.contains(peer.addr()) {
            self.add_bootstrap(peer.addr().clone());
            self.save().await;
        }

        // 2. add to kad.
        if self
            .dhts
            .search(&peer_id)
            .and_then(|(_k, _v, is_it)| if is_it { Some(()) } else { None })
            .is_none()
        {
            self.dhts
                .add(peer_id, KadValue(sender, stream_sender, peer));
            true
        } else {
            false
        }
    }

    /// Step:
    /// 1. remove from kad;
    pub fn peer_remove(
        &mut self,
        peer_id: &PeerId,
    ) -> Option<(Sender<SessionMessage>, Sender<EndpointMessage>, Peer)> {
        self.dhts.remove(peer_id).map(|v| (v.0, v.1, v.2))
    }

    /// Disconnect Step:
    /// 1. remove from bootstrap.
    pub async fn peer_disconnect(&mut self, addr: &SocketAddr) {
        let mut d: Option<usize> = None;
        for (k, i) in self.bootstraps.iter().enumerate() {
            if i == addr {
                d = Some(k);
            }
        }

        if let Some(i) = d {
            self.bootstraps.remove(i);
            self.save().await;
        }
    }

    /// PeerDisconnect Step:
    /// 1. remove from white_list;
    /// 2. remove from stables.
    pub fn stable_remove(&mut self, peer_id: &PeerId) -> Option<Sender<SessionMessage>> {
        self.remove_white_peer(peer_id);
        self.stables.remove(peer_id).map(|v| (v.0).0)
    }

    /// Peerl leave Step:
    /// 1. remove from stables.
    pub fn stable_leave(&mut self, peer_id: &PeerId) {
        self.stables.remove(peer_id);
    }

    /// Peer stable connect ok Step:
    /// 1. add to bootstrap;
    /// 2. add to stables;
    pub fn stable_add(
        &mut self,
        peer_id: PeerId,
        sender: Sender<SessionMessage>,
        stream_sender: Sender<EndpointMessage>,
        peer: Peer,
        is_direct: bool,
    ) {
        match self.stables.get_mut(&peer_id) {
            Some((KadValue(s, ss, p), direct)) => {
                let _ = s.try_send(SessionMessage::Close);
                *s = sender;
                *ss = stream_sender;
                *p = peer;
                *direct = is_direct;
            }
            None => {
                self.stables
                    .insert(peer_id, (KadValue(sender, stream_sender, peer), is_direct));
            }
        }
    }

    pub fn get_tmp_stable(&self, peer_id: &PeerId) -> Option<&Sender<SessionMessage>> {
        self.tmp_stables.get(peer_id).map(|v| &v.0).or(self
            .dht_get(peer_id)
            .map(|v| if v.2 { Some(v.0) } else { None })
            .flatten())
    }

    pub fn get_endpoint_with_tmp(&self, peer_id: &PeerId) -> Option<&Sender<EndpointMessage>> {
        self.tmp_stables.get(peer_id).map(|v| &v.1).or(self
            .get(peer_id)
            .map(|v| if v.2 { Some(v.1) } else { None })
            .flatten())
    }

    pub fn remove_tmp_stable(&mut self, peer_id: &PeerId) {
        let _ = self.tmp_stables.remove(peer_id);
    }

    pub fn add_tmp_stable(
        &mut self,
        peer_id: PeerId,
        sender: Sender<SessionMessage>,
        stream_sender: Sender<EndpointMessage>,
    ) {
        self.tmp_stables.insert(peer_id, (sender, stream_sender));
    }
}

// Black and white list.
impl PeerList {
    pub fn bootstrap(&self) -> &Vec<SocketAddr> {
        &self.bootstraps
    }

    pub fn add_bootstrap(&mut self, addr: SocketAddr) {
        if !self.bootstraps.contains(&addr) {
            self.bootstraps.push(addr);
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
