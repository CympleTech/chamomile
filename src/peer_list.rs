use smol::{channel::Sender, fs, io::Result};
use std::collections::HashMap;
use std::io::BufRead;
use std::iter::Iterator;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use chamomile_types::types::{new_io_error, PeerId};

use crate::kad::{DoubleKadTree, KadValue};
use crate::peer::Peer;
use crate::session::SessionMessage;
use crate::transports::EndpointMessage;

/// PeerList.
/// contains: dhts(KadTree) & stables(HashMap)
pub(crate) struct PeerList {
    save_path: PathBuf,
    bootstraps: Vec<SocketAddr>,
    allows: (Vec<PeerId>, Vec<SocketAddr>),
    blocks: (Vec<PeerId>, Vec<IpAddr>),

    /// PeerId => KadValue(Sender<Sessionmessage>, Sender<EndpointMessage>, Peer)
    dhts: DoubleKadTree,
    /// PeerId => KadValue(Sender<SessionMessage>, Sender<EndpointMessage>, Peer)
    stables: HashMap<PeerId, (KadValue, bool)>,
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
        allows: (Vec<PeerId>, Vec<SocketAddr>),
        blocks: (Vec<PeerId>, Vec<IpAddr>),
    ) -> Self {
        let default_socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let mut bootstraps = allows.1.clone();
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
                    allows: allows,
                    blocks: blocks,
                    dhts: DoubleKadTree::new(peer_id, default_socket),
                    stables: HashMap::new(),
                }
            }
            Err(_) => PeerList {
                save_path,
                bootstraps,
                allows: allows,
                blocks: blocks,
                dhts: DoubleKadTree::new(peer_id, default_socket),
                stables: HashMap::new(),
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stables.is_empty() && self.dhts.is_empty()
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

    /// search in stable list. result is stream channel sender.
    pub fn get_stable_stream(&self, peer_id: &PeerId) -> Option<&Sender<EndpointMessage>> {
        self.stable_get(peer_id)
            .map(|(_ss, stream, is_it)| if is_it { Some(stream) } else { None })
            .flatten()
    }

    pub fn next_closest(&self, target: &PeerId, prev: &PeerId) -> Option<&Sender<SessionMessage>> {
        self.stables
            .get(target)
            .map(|v| &(v.0).0)
            .or(self.dhts.id_next_closest(target, prev).map(|v| &v.0))
    }

    pub fn _ip_next_closest(
        &self,
        ip: &SocketAddr,
        prev: &SocketAddr,
    ) -> Option<&Sender<SessionMessage>> {
        self.dhts._ip_next_closest(ip, prev).map(|v| &v.0)
    }

    /// search in dht table.
    pub fn dht_get(
        &self,
        peer_id: &PeerId,
    ) -> Option<(&Sender<SessionMessage>, &Sender<EndpointMessage>, bool)> {
        self.dhts
            .search(peer_id)
            .map(|(v, is_it)| (&v.0, &v.1, is_it))
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

    /// check stable is relay.
    pub fn is_relay(&self, peer_id: &PeerId) -> Option<&Sender<SessionMessage>> {
        self.stables
            .get(peer_id)
            .map(|v| if !v.1 { Some(&(v.0).0) } else { None })
            .flatten()
    }

    /// get in DHT help
    pub fn help_dht(&self, peer_id: &PeerId) -> Vec<Peer> {
        // TODO better closest peers

        let mut peers: HashMap<&PeerId, &Peer> = HashMap::new();
        for key in self.dhts.keys().into_iter() {
            if &key == peer_id {
                continue;
            }
            if let Some((KadValue(_, _, peer), is_it)) = self.dhts.search(&key) {
                if is_it {
                    peers.insert(peer.id(), peer);
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
    /// 1. remove from kad;
    pub fn remove_peer(
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

    /// Peer leave Step:
    /// 1. remove from stables.
    pub fn stable_leave(&mut self, peer_id: &PeerId) {
        self.stables.remove(peer_id);
    }

    /// Step:
    /// 1. add to boostraps;
    /// 2. add to kad.
    pub async fn add_dht(&mut self, v: KadValue) -> bool {
        // 1. add to boostraps.
        if v.2.is_pub() && !self.bootstraps.contains(v.2.addr()) {
            self.add_bootstrap(v.2.addr().clone());
            self.save().await;
        }

        // 2. add to kad.
        if self.dhts.add(v) {
            true
        } else {
            false
        }
    }

    /// Peer stable connect ok Step:
    /// 1. add to bootstrap;
    /// 2. add to stables;
    pub fn add_stable(&mut self, peer_id: PeerId, v: KadValue, is_direct: bool) {
        match self.stables.get_mut(&peer_id) {
            Some((KadValue(s, ss, p), direct)) => {
                let _ = s.try_send(SessionMessage::Close);
                let KadValue(sender, stream, peer) = v;
                *s = sender;
                *ss = stream;
                *p = peer;
                *direct = is_direct;
            }
            None => {
                self.add_allow_peer(peer_id);
                self.stables.insert(peer_id, (v, is_direct));
            }
        }
    }

    pub fn stable_to_dht(&mut self, peer_id: &PeerId) -> Result<()> {
        self.remove_allow_peer(peer_id);
        if let Some((v, is_direct)) = self.stables.remove(peer_id) {
            if is_direct {
                if self.dhts.add(v) {
                    return Ok(());
                }
            }
        }
        Err(new_io_error("stable is closed"))
    }

    pub fn dht_to_stable(&mut self, peer_id: &PeerId) -> Result<()> {
        if let Some(v) = self.dhts.remove(peer_id) {
            self.add_allow_peer(*peer_id);
            self.stables.insert(*peer_id, (v, true));
            Ok(())
        } else {
            Err(new_io_error("DHT is closed"))
        }
    }
}

// Block and allow list.
impl PeerList {
    pub fn bootstrap(&self) -> &Vec<SocketAddr> {
        &self.bootstraps
    }

    pub fn add_bootstrap(&mut self, addr: SocketAddr) {
        if !self.bootstraps.contains(&addr) {
            self.bootstraps.push(addr);
        }
    }

    pub fn _is_allow_peer(&self, peer: &PeerId) -> bool {
        self.allows.0.contains(peer)
    }

    pub fn _is_allow_addr(&self, addr: &SocketAddr) -> bool {
        self.allows.1.contains(addr)
    }

    pub fn add_allow_peer(&mut self, peer: PeerId) {
        if !self.allows.0.contains(&peer) {
            self.allows.0.push(peer)
        }
    }

    pub fn _add_allow_addr(&mut self, addr: SocketAddr) {
        if !self.allows.1.contains(&addr) {
            self.allows.1.push(addr)
        }
    }

    pub fn remove_allow_peer(&mut self, peer: &PeerId) -> Option<PeerId> {
        let pos = match self.allows.0.iter().position(|x| *x == *peer) {
            Some(x) => x,
            None => return None,
        };
        Some(self.allows.0.remove(pos))
    }

    pub fn _remove_allow_addr(&mut self, addr: &SocketAddr) -> Option<SocketAddr> {
        let pos = match self.allows.1.iter().position(|x| *x == *addr) {
            Some(x) => x,
            None => return None,
        };
        Some(self.allows.1.remove(pos))
    }

    pub fn is_block_peer(&self, peer: &PeerId) -> bool {
        self.blocks.0.contains(peer)
    }

    pub fn is_block_addr(&self, addr: &SocketAddr) -> bool {
        self.blocks.1.contains(&addr.ip())
    }

    pub fn _add_block_peer(&mut self, peer: PeerId) {
        if !self.blocks.0.contains(&peer) {
            self.blocks.0.push(peer)
        }
    }

    pub fn _add_block_addr(&mut self, addr: SocketAddr) {
        if !self.blocks.1.contains(&addr.ip()) {
            self.blocks.1.push(addr.ip())
        }
    }

    pub fn _remove_block_peer(&mut self, peer: &PeerId) -> Option<PeerId> {
        let pos = match self.blocks.0.iter().position(|x| *x == *peer) {
            Some(x) => x,
            None => return None,
        };
        Some(self.blocks.0.remove(pos))
    }

    pub fn _remove_block_addr(&mut self, addr: &SocketAddr) -> Option<IpAddr> {
        let pos = match self.blocks.1.iter().position(|x| *x == addr.ip()) {
            Some(x) => x,
            None => return None,
        };
        Some(self.blocks.1.remove(pos))
    }
}
