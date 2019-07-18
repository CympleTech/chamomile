use actix::prelude::Recipient;
use multiaddr::Multiaddr;
use rckad::KadTree;
use std::collections::HashMap;

use super::session::SessionSend;
use crate::core::peer_id::PeerID;

#[derive(Clone)]
pub(crate) struct NodeAddr {
    multiaddr: Multiaddr,
    session: Recipient<SessionSend>,
}

impl NodeAddr {
    pub fn new(multiaddr: Multiaddr, session: Recipient<SessionSend>) -> Self {
        NodeAddr { multiaddr, session }
    }

    pub fn multiaddr(&self) -> &Multiaddr {
        &self.multiaddr
    }

    pub fn session(&self) -> &Recipient<SessionSend> {
        &self.session
    }
}

/// PeerList
/// contains: peers(DHT) & tmp_peers(HashMap)
pub(crate) struct PeerList {
    peers: KadTree<PeerID, Option<NodeAddr>>,
    tmps: HashMap<PeerID, NodeAddr>,
}

impl PeerList {
    pub fn init(self_peer_id: PeerID) -> Self {
        PeerList {
            peers: KadTree::new(self_peer_id, None),
            tmps: HashMap::new(),
        }
    }

    pub(crate) fn load() {}

    pub fn all(&self) {}

    /// get in DHT, DHT closest.
    pub fn get(&self, peer_id: &PeerID) -> Option<&NodeAddr> {
        self.peers
            .search(peer_id)
            .and_then(|(k, ref v, is_it)| v.as_ref())
    }

    /// get in DHT (not closest).
    pub fn get_it(&self, peer_id: &PeerID) -> Option<&NodeAddr> {
        self.peers
            .search(peer_id)
            .and_then(|(k, v, is_it)| if is_it { v.as_ref() } else { None })
    }

    /// get in DHT, tmp_peers,  DHT closest
    pub fn get_all(&self, peer_id: &PeerID) -> Option<&NodeAddr> {
        self.peers.search(peer_id).and_then(|(k, v, is_it)| {
            if is_it {
                v.as_ref()
            } else {
                self.tmps.get(peer_id).or(v.as_ref())
            }
        })
    }

    pub fn contains(&self, peer_id: &PeerID) -> bool {
        self.peers.contains(peer_id)
    }

    pub fn remove(&mut self, peer_id: &PeerID) -> Option<NodeAddr> {
        let r1 = self.peers.remove(peer_id).and_then(|v| v);
        let r2 = self.remove_tmp_peer(peer_id);
        r1.or(r2)
    }

    pub fn add_tmp_peer(&mut self, peer_id: PeerID, node: NodeAddr) {
        self.tmps
            .entry(peer_id)
            .and_modify(|m| *m = node.clone())
            .or_insert(node);
    }

    pub fn remove_tmp_peer(&mut self, peer_id: &PeerID) -> Option<NodeAddr> {
        self.tmps.remove(peer_id)
    }

    pub fn stabilize_tmp_peer(&mut self, peer_id: PeerID) {
        self.tmps
            .remove(&peer_id)
            .map(|m| self.peers.add(peer_id, Some(m)));
    }
}
