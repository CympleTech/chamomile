use std::collections::HashMap;
use std::io::BufRead;
use std::iter::Iterator;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use tokio::{fs, io::Result, sync::mpsc::Sender};

use chamomile_types::{types::new_io_error, Peer, PeerId};

use crate::kad::{DoubleKadTree, KadValue};
use crate::session::SessionMessage;
use crate::transports::EndpointMessage;

/// PeerList.
/// contains: dhts(KadTree) & stables(HashMap)
pub(crate) struct PeerList {
    save_path: PathBuf,
    allows: Vec<Peer>,
    blocks: (Vec<PeerId>, Vec<IpAddr>),

    /// PeerId => KadValue(Sender<Sessionmessage>, Sender<EndpointMessage>, Peer)
    dhts: DoubleKadTree,
    /// PeerId => KadValue(Sender<SessionMessage>, Sender<EndpointMessage>, Peer)
    stables: HashMap<PeerId, (KadValue, bool)>,
    /// Own assist-ids
    owns: Vec<PeerId>,
}

impl PeerList {
    pub async fn save(&self) {
        let mut file_string = String::new();
        for addr in &self.allows {
            file_string = format!("{}\n{}", file_string, addr.to_multiaddr_string());
        }
        let _ = fs::write(&self.save_path, file_string).await;
    }

    pub fn load(
        peer_id: PeerId,
        assist_id: PeerId,
        save_path: PathBuf,
        mut allows: Vec<Peer>,
        blocks: (Vec<PeerId>, Vec<IpAddr>),
    ) -> Self {
        let default_socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        match std::fs::File::open(&save_path) {
            Ok(file) => {
                let addrs = std::io::BufReader::new(file).lines();
                for addr in addrs {
                    if let Ok(addr) = addr {
                        if let Ok(p) = Peer::from_multiaddr_string(&addr) {
                            let mut is_new = true;
                            for ap in allows.iter() {
                                if ap.socket == p.socket {
                                    is_new = false;
                                }
                            }
                            if is_new {
                                allows.push(p);
                            }
                        }
                    }
                }
                PeerList {
                    save_path,
                    allows: allows,
                    blocks: blocks,
                    dhts: DoubleKadTree::new(peer_id, assist_id, default_socket),
                    stables: HashMap::new(),
                    owns: vec![],
                }
            }
            Err(_) => PeerList {
                save_path,
                allows: allows,
                blocks: blocks,
                dhts: DoubleKadTree::new(peer_id, assist_id, default_socket),
                stables: HashMap::new(),
                owns: vec![],
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
    pub fn help_dht(&self, _peer_id: &PeerId) -> Vec<Peer> {
        // TODO better closest peers

        let mut peers = vec![];
        for (_, (_, v)) in &self.dhts.values {
            for va in v.iter() {
                peers.push(va.2);
            }
        }

        for (_, v) in self.stables.iter() {
            peers.push((v.0).2);
        }

        peers
    }

    /// Step:
    /// 1. remove from kad;
    pub fn remove_peer(&mut self, peer_id: &PeerId, assist_id: &PeerId) {
        self.dhts.remove(peer_id, assist_id);
    }

    /// Disconnect Step:
    /// 1. remove from bootstrap.
    pub async fn peer_disconnect(&mut self, addr: &SocketAddr) {
        let mut d: Option<usize> = None;
        for (k, i) in self.allows.iter().enumerate() {
            if &i.socket == addr {
                d = Some(k);
            }
        }

        if let Some(i) = d {
            self.allows.remove(i);
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
        if v.2.is_pub && !self.allows.contains(&v.2) {
            self.add_bootstrap(v.2);
            self.save().await;
        }

        // 2. add to kad.
        if self.dhts.add(v) {
            true
        } else {
            false
        }
    }

    /// add inner-own device.
    pub fn add_own(&mut self, assist_id: PeerId, v: KadValue, is_direct: bool) {
        if !self.owns.contains(&assist_id) {
            self.owns.push(assist_id);
            self.add_stable(assist_id, v, is_direct);
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

    pub fn dht_to_stable(&mut self, peer_id: &PeerId, a_id: &PeerId) -> Result<()> {
        if let Some(mut v) = self.dhts.take(peer_id, a_id) {
            // only use one in stable.
            if let Some(va) = v.pop() {
                self.add_allow_peer(*peer_id);
                self.stables.insert(*peer_id, (va, true));
            }
            Ok(())
        } else {
            Err(new_io_error("DHT is closed"))
        }
    }
}

// Block and allow list.
impl PeerList {
    pub fn own(&self) -> &[PeerId] {
        &self.owns
    }

    pub fn bootstrap(&self) -> Vec<&Peer> {
        self.allows
            .iter()
            .filter_map(|p| if p.effective_socket() { Some(p) } else { None })
            .collect()
    }

    pub fn add_bootstrap(&mut self, peer: Peer) {
        let mut is_new = true;
        for ap in self.allows.iter() {
            if ap.socket == peer.socket {
                is_new = false;
            }
        }
        if is_new {
            self.allows.push(peer);
        }
    }

    pub fn add_allow_peer(&mut self, pid: PeerId) {
        let mut is_new = true;
        for ap in self.allows.iter() {
            if ap.id == pid {
                is_new = false;
            }
        }
        if is_new {
            self.allows.push(Peer::peer(pid));
        }
    }

    pub fn remove_allow_peer(&mut self, peer: &PeerId) -> Option<Peer> {
        let pos = match self.allows.iter().position(|x| &x.id == peer) {
            Some(x) => x,
            None => return None,
        };
        Some(self.allows.remove(pos))
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
