use smol::{channel::Sender, fs};
use std::collections::HashMap;
use std::io::BufRead;
use std::iter::Iterator;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use chamomile_types::types::PeerId;

use crate::kad::{DoubleKadTree, KadValue};
use crate::peer::Peer;
use crate::session::SessionMessage;
use crate::transports::EndpointMessage;

/// PeerList
/// contains: dhts(DHT) & tmp_dhts(HashMap)
pub(crate) struct PeerList {
    save_path: PathBuf,
    bootstraps: Vec<SocketAddr>,
    allows: (Vec<PeerId>, Vec<SocketAddr>),
    blocks: (Vec<PeerId>, Vec<IpAddr>),

    /// PeerId => KadValue(Sender<Sessionmessage>, Sender<EndpointMessage>, Peer)
    dhts: DoubleKadTree,
    /// PeerId => KadValue(Sender<SessionMessage>, Sender<EndpointMessage>, Peer)
    stables: HashMap<PeerId, (KadValue, bool)>,

    /// queue for connect to ip addr. if has one, not send aggin.
    _buffer_queue_ip: Vec<SocketAddr>,
    /// queue for stable connect to peer id. if has one, add to queue buffer.
    buffer_queue_peer_id: HashMap<PeerId, Vec<(u64, Vec<u8>)>>,
    /// tmp stable waiting outside to stable result.
    buffer_tmp_stable: HashMap<PeerId, (Sender<SessionMessage>, Sender<EndpointMessage>)>,
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
                    _buffer_queue_ip: vec![],
                    buffer_queue_peer_id: HashMap::new(),
                    buffer_tmp_stable: HashMap::new(),
                }
            }
            Err(_) => PeerList {
                save_path,
                bootstraps,
                allows: allows,
                blocks: blocks,
                dhts: DoubleKadTree::new(peer_id, default_socket),
                stables: HashMap::new(),
                _buffer_queue_ip: vec![],
                buffer_queue_peer_id: HashMap::new(),
                buffer_tmp_stable: HashMap::new(),
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

    pub fn peer_next_closest(
        &self,
        peer_id: &PeerId,
        prev: &PeerId,
    ) -> Option<&Sender<SessionMessage>> {
        self.stables
            .get(peer_id)
            .map(|v| &(v.0).0)
            .or(self.dhts.peer_next_closest(peer_id, prev).map(|v| &v.0))
    }

    pub fn ip_next_closest(
        &self,
        ip: &SocketAddr,
        prev: &SocketAddr,
    ) -> Option<&Sender<SessionMessage>> {
        self.dhts.ip_next_closest(ip, prev).map(|v| &v.0)
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

    /// if peer has stable connected in peer list.
    pub fn stable_contains(&self, peer_id: &PeerId) -> bool {
        self.stables.contains_key(peer_id)
    }

    pub fn stable_check_relay(&self, peer_id: &PeerId) -> Option<&Sender<SessionMessage>> {
        self.stables
            .get(peer_id)
            .map(|v| if !v.1 { Some(&(v.0).0) } else { None })
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
    /// 1. add to boostraps;
    /// 2. add to kad.
    pub async fn peer_add(
        &mut self,
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
        if self.dhts.add(KadValue(sender, stream_sender, peer)) {
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
        self.remove_tmp_stable(peer_id);
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
    /// 1. remove from allowlist;
    /// 2. remove from stables.
    pub fn stable_remove(&mut self, peer_id: &PeerId) -> bool {
        self.remove_allow_peer(peer_id);
        // check save to DHT.
        if let Some((v, is_direct)) = self.stables.remove(peer_id) {
            if is_direct {
                if self.dhts.add(v) {
                    return false;
                }
            }
        }
        true
    }

    /// Peer leave Step:
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

    pub fn _add_buffer_ip(&mut self, ip: &SocketAddr) -> bool {
        if self._buffer_queue_ip.contains(ip) {
            false
        } else {
            self._buffer_queue_ip.push(*ip);
            true
        }
    }

    pub fn _remove_buffer_ip(&mut self, ip: &SocketAddr) {
        if let Some(pos) = self._buffer_queue_ip.iter().position(|x| x == ip) {
            self._buffer_queue_ip.remove(pos);
        }
    }

    pub fn contains_buffer_peer_id(&mut self, peer_id: &PeerId) -> bool {
        debug!(
            "DEBUG: ======= BUFFER PEER: {:?}",
            self.buffer_queue_peer_id.keys()
        );
        if self.buffer_queue_peer_id.contains_key(peer_id) {
            true
        } else {
            self.buffer_queue_peer_id.insert(*peer_id, vec![]);
            false
        }
    }

    pub fn add_buffer_peer_id(&mut self, peer_id: &PeerId, tid: u64, data: Vec<u8>) {
        if let Some(v) = self.buffer_queue_peer_id.get_mut(peer_id) {
            v.push((tid, data));
        }
    }

    pub fn remove_buffer_peer_id(&mut self, peer_id: &PeerId) -> Option<Vec<(u64, Vec<u8>)>> {
        self.buffer_queue_peer_id.remove(peer_id)
    }

    pub fn get_tmp_stable(&self, peer_id: &PeerId) -> Option<&Sender<SessionMessage>> {
        self.buffer_tmp_stable.get(peer_id).map(|v| &v.0).or(self
            .dht_get(peer_id)
            .map(|v| if v.2 { Some(v.0) } else { None })
            .flatten())
    }

    pub fn get_endpoint_with_tmp(&self, peer_id: &PeerId) -> Option<&Sender<EndpointMessage>> {
        self.buffer_tmp_stable.get(peer_id).map(|v| &v.1).or(self
            .get(peer_id)
            .map(|v| if v.2 { Some(v.1) } else { None })
            .flatten())
    }

    pub fn remove_tmp_stable(&mut self, peer_id: &PeerId) {
        let _ = self.buffer_tmp_stable.remove(peer_id);
    }

    pub fn add_tmp_stable(
        &mut self,
        peer_id: PeerId,
        sender: Sender<SessionMessage>,
        stream_sender: Sender<EndpointMessage>,
    ) {
        self.buffer_tmp_stable
            .insert(peer_id, (sender, stream_sender));
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

    pub fn is_allow_peer(&self, peer: &PeerId) -> bool {
        self.allows.0.contains(peer)
    }

    pub fn is_allow_addr(&self, addr: &SocketAddr) -> bool {
        self.allows.1.contains(addr)
    }

    pub fn add_allow_peer(&mut self, peer: PeerId) {
        if !self.allows.0.contains(&peer) {
            self.allows.0.push(peer)
        }
    }

    pub fn add_allow_addr(&mut self, addr: SocketAddr) {
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

    pub fn remove_allow_addr(&mut self, addr: &SocketAddr) -> Option<SocketAddr> {
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

    pub fn add_block_peer(&mut self, peer: PeerId) {
        if !self.blocks.0.contains(&peer) {
            self.blocks.0.push(peer)
        }
    }

    pub fn add_block_addr(&mut self, addr: SocketAddr) {
        if !self.blocks.1.contains(&addr.ip()) {
            self.blocks.1.push(addr.ip())
        }
    }

    pub fn remove_block_peer(&mut self, peer: &PeerId) -> Option<PeerId> {
        let pos = match self.blocks.0.iter().position(|x| *x == *peer) {
            Some(x) => x,
            None => return None,
        };
        Some(self.blocks.0.remove(pos))
    }

    pub fn remove_block_addr(&mut self, addr: &SocketAddr) -> Option<IpAddr> {
        let pos = match self.blocks.1.iter().position(|x| *x == addr.ip()) {
            Some(x) => x,
            None => return None,
        };
        Some(self.blocks.1.remove(pos))
    }
}
