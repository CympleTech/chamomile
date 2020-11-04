use rckad::KadTree;
use smol::{channel::Sender, fs};
use std::collections::HashMap;
use std::io::BufRead;
use std::iter::Iterator;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use chamomile_types::types::PeerId;

use crate::peer::Peer;
use crate::session::SessionSendMessage;

/// PeerList
/// contains: peers(DHT) & tmp_peers(HashMap)
pub(crate) struct PeerList {
    save_path: PathBuf,
    peers: KadTree<PeerId, Option<(Sender<SessionSendMessage>, Peer)>>,
    stables: HashMap<PeerId, (Sender<SessionSendMessage>, Peer)>,
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
                    peers: KadTree::new(peer_id, None),
                    stables: HashMap::new(),
                    whites: whites,
                    blacks: blacks,
                }
            }
            Err(_) => PeerList {
                save_path,
                bootstraps,
                peers: KadTree::new(peer_id, None),
                stables: HashMap::new(),
                whites: whites,
                blacks: blacks,
            },
        }
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

    pub fn stable_all(&self) -> Vec<(PeerId, &Sender<SessionSendMessage>)> {
        self.stables.iter().map(|(k, v)| (*k, &v.0)).collect()
    }

    /// get in Stables, DHT, DHT closest.
    pub fn get(&self, peer_id: &PeerId) -> Option<&Sender<SessionSendMessage>> {
        self.stables.get(peer_id).map(|v| &v.0).or(self
            .peers
            .search(peer_id)
            .map(|(_k, ref v, _is_it)| v.as_ref().map(|vv| &vv.0))
            .flatten())
    }

    /// get in DHT (not closest).
    pub fn get_it(&self, peer_id: &PeerId) -> Option<&Sender<SessionSendMessage>> {
        self.stables.get(peer_id).map(|v| &v.0).or(self
            .peers
            .search(peer_id)
            .map(|(_k, v, is_it)| {
                if is_it {
                    v.as_ref().map(|vv| &vv.0)
                } else {
                    None
                }
            })
            .flatten())
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

    pub fn contains_stable(&self, peer_id: &PeerId) -> bool {
        self.stables.contains_key(peer_id)
    }

    /// Step:
    /// 1. add to boostraps;
    /// 2. add to kad.
    pub async fn peer_add(
        &mut self,
        peer_id: PeerId,
        sender: Sender<SessionSendMessage>,
        peer: Peer,
    ) -> bool {
        // 1. add to boostraps.
        if !self.bootstraps.contains(peer.addr()) {
            self.add_bootstrap(peer.addr().clone());
            self.save().await;
        }

        // 2. add to kad.
        if self
            .peers
            .search(&peer_id)
            .and_then(|(_k, _v, is_it)| if is_it { Some(()) } else { None })
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
    pub fn peer_remove(&mut self, peer_id: &PeerId) -> Option<(Sender<SessionSendMessage>, Peer)> {
        self.peers.remove(peer_id).and_then(|v| v)
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
    pub fn stable_remove(
        &mut self,
        peer_id: &PeerId,
    ) -> Option<(Sender<SessionSendMessage>, Peer)> {
        self.remove_white_peer(peer_id);
        self.stables.remove(peer_id)
    }

    /// Peerl leave Step:
    /// 1. remove from stables.
    pub fn stable_leave(&mut self, peer_id: &PeerId) {
        self.stables.remove(peer_id);
    }

    /// Peer stable connect ok Step:
    /// 1. add to bootstrap;
    /// 2. add to stables;
    pub fn stable_add(&mut self, peer_id: PeerId, sender: Sender<SessionSendMessage>, peer: Peer) {
        if !self.stables.contains_key(&peer_id) {
            self.stables.insert(peer_id, (sender, peer));
        } else {
            let _ = sender.try_send(SessionSendMessage::Close);
        }
    }
}
