use bit_vec::BitVec;
use chamomile_types::types::PeerId;
use core::cmp::Ordering;
use rand::Rng;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::mpsc::Sender;

use crate::peer::Peer;
use crate::session::SessionMessage;
use crate::transports::EndpointMessage;

trait Key: Eq + Clone {
    const KEY_LENGTH: usize;
    fn distance(&self) -> Distance;
    fn calc_distance(base: &Self, target: &Self) -> Distance {
        let base = base.distance();
        let target = target.distance();
        base.xor(&target, Self::KEY_LENGTH)
    }
}

impl Key for PeerId {
    const KEY_LENGTH: usize = 256;
    fn distance(&self) -> Distance {
        // 256-bit
        Distance(BitVec::from_bytes(self.as_bytes()))
    }
}

impl Key for SocketAddr {
    const KEY_LENGTH: usize = 144;
    fn distance(&self) -> Distance {
        let ip_bytes: [u8; 16] = match self {
            SocketAddr::V4(ipv4) => ipv4.ip().to_ipv6_mapped().octets(),
            SocketAddr::V6(ipv6) => ipv6.ip().octets(),
        };
        let port_bytes: [u8; 2] = self.port().to_le_bytes();
        // 144-bit = 128 + 16
        Distance(BitVec::from_bytes(
            &[&ip_bytes[..], &port_bytes[..]].concat(),
        ))
    }
}

const MAX_LEVEL: usize = 8;

// max peer-id is 4 * 256 = 1024
// max ip-address is 4 * 128 = 512
const K_BUCKET: usize = 4;

pub(crate) struct KadValue(
    pub Sender<SessionMessage>,
    pub Sender<EndpointMessage>,
    pub Peer,
);

pub(crate) struct DoubleKadTree {
    values: HashMap<u32, KadValue>,
    peers: KadTree<PeerId>,
    ips: KadTree<SocketAddr>,
}

struct KadTree<K: Key> {
    root_key: K,
    left: TreeNode<K>,
    right: TreeNode<K>,
}

type TreeNode<K> = Option<Box<Node<K>>>;

struct Node<K: Key> {
    left: TreeNode<K>,
    right: TreeNode<K>,
    list: Vec<Cell<K>>,
}

struct Cell<K>(K, u32, Distance);

impl DoubleKadTree {
    pub fn new(root_peer: PeerId, root_ip: SocketAddr) -> Self {
        DoubleKadTree {
            peers: KadTree::new(root_peer),
            ips: KadTree::new(root_ip),
            values: HashMap::new(),
        }
    }

    pub fn add(&mut self, value: KadValue) -> bool {
        let mut rng = rand::thread_rng();
        let value_key = rng.gen::<u32>();
        let peer_id = value.2.id().clone();
        let ip_addr = value.2.addr().clone();
        let (is_ok, removed) = self.peers.add(peer_id, value_key);
        for i in removed {
            self.values.remove(&i);
        }
        if is_ok {
            self.ips.add(ip_addr, value_key);
            self.values.insert(value_key, value);
            true
        } else {
            false
        }
    }

    pub fn id_next_closest(&self, key: &PeerId, prev: &PeerId) -> Option<&KadValue> {
        self.peers
            .next_closest(key, prev)
            .map(|k| self.values.get(k))
            .flatten()
    }

    pub fn _ip_next_closest(&self, key: &SocketAddr, prev: &SocketAddr) -> Option<&KadValue> {
        self.ips
            .next_closest(key, prev)
            .map(|k| self.values.get(k))
            .flatten()
    }

    pub fn search(&self, key: &PeerId) -> Option<(&KadValue, bool)> {
        self.peers
            .search(key)
            .map(|(_, k, is_it)| self.values.get(k).map(|v| (v, is_it)))
            .flatten()
    }

    pub fn remove(&mut self, key: &PeerId) -> Option<KadValue> {
        if let Some(k) = self.peers.remove(key) {
            if let Some(value) = self.values.remove(&k) {
                self.ips.remove(value.2.addr());
                return Some(value);
            }
        }
        None
    }

    pub fn contains(&self, key: &PeerId) -> bool {
        self.peers.contains(key)
    }

    pub fn keys(&self) -> Vec<PeerId> {
        self.peers.keys()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}

impl<K: Key> KadTree<K> {
    fn new(key: K) -> Self {
        KadTree {
            root_key: key,
            left: None,
            right: None,
        }
    }

    fn add(&mut self, key: K, value: u32) -> (bool, Vec<u32>) {
        let distance = K::calc_distance(&self.root_key, &key);

        if distance.get(0) {
            if self.right.is_none() {
                self.right = Some(Box::new(Node::default()));
            }
            self.right
                .as_mut()
                .and_then(|v| Some(v.insert(Cell(key, value, distance), 1, K_BUCKET)))
                .unwrap() // safe checked.
        } else {
            if self.left.is_none() {
                self.left = Some(Box::new(Node::default()));
            }
            self.left
                .as_mut()
                .and_then(|v| Some(v.insert(Cell(key, value, distance), 1, K_BUCKET)))
                .unwrap() // safe checked.
        }
    }

    fn next_closest(&self, key: &K, prev: &K) -> Option<&u32> {
        self.search(key)
            .map(|v| if v.0 == prev { None } else { Some(v.1) })
            .flatten()
    }

    fn search(&self, key: &K) -> Option<(&K, &u32, bool)> {
        let distance = K::calc_distance(&self.root_key, &key);

        if distance.get(0) {
            if self.right.is_none() {
                if self.left.is_none() {
                    None
                } else {
                    self.left
                        .as_ref()
                        .and_then(|v| Some(v.search(key, &distance, 1)))
                        .unwrap() // safe checked.
                }
            } else {
                self.right
                    .as_ref()
                    .and_then(|v| Some(v.search(key, &distance, 1)))
                    .unwrap() // safe chekced.
            }
        } else {
            if self.left.is_none() {
                if self.right.is_none() {
                    None
                } else {
                    self.right
                        .as_ref()
                        .and_then(|v| Some(v.search(key, &distance, 1)))
                        .unwrap() // safe checked.
                }
            } else {
                self.left
                    .as_ref()
                    .and_then(|v| Some(v.search(key, &distance, 1)))
                    .unwrap() // safe checked.
            }
        }
    }

    fn remove(&mut self, key: &K) -> Option<u32> {
        let distance = K::calc_distance(&self.root_key, &key);
        if distance.get(0) {
            self.right
                .as_mut()
                .and_then(|v| v.remove(key, &distance, 1))
        } else {
            self.left.as_mut().and_then(|v| v.remove(key, &distance, 1))
        }
    }

    fn contains(&self, key: &K) -> bool {
        if let Some((_, _, true)) = self.search(key) {
            true
        } else {
            false
        }
    }

    fn keys(&self) -> Vec<K> {
        let mut vec = Vec::new();
        if self.left.is_some() {
            self.left.as_ref().unwrap().keys(&mut vec); // safe checked.
        }
        if self.right.is_some() {
            self.right.as_ref().unwrap().keys(&mut vec); // safe checked.
        }
        vec
    }

    fn is_empty(&self) -> bool {
        if self.left.is_some() {
            if !self.left.as_ref().unwrap().is_empty() {
                return false;
            }
        }
        if self.right.is_some() {
            if !self.right.as_ref().unwrap().is_empty() {
                return false;
            }
        }

        true
    }
}

impl<K: Key> Node<K> {
    fn default() -> Self {
        Node {
            left: None,
            right: None,
            list: Vec::new(),
        }
    }

    fn insert(&mut self, mut cell: Cell<K>, index: usize, k_bucket: usize) -> (bool, Vec<u32>) {
        if self.right.is_some() || self.left.is_some() {
            if cell.2.get(index) {
                if self.right.is_none() {
                    self.right = Some(Box::new(Node::default()));
                }
                self.right
                    .as_mut()
                    .and_then(|v| Some(v.insert(cell, index + 1, k_bucket)))
                    .unwrap() // safe checked.
            } else {
                if self.left.is_none() {
                    self.left = Some(Box::new(Node::default()));
                }
                self.left
                    .as_mut()
                    .and_then(|v| Some(v.insert(cell, index + 1, k_bucket)))
                    .unwrap() // safe checked.
            }
        } else {
            let mut need_deleted = usize::MAX;
            let mut removed = vec![];
            for (i, c) in self.list.iter().enumerate() {
                if c == &cell {
                    need_deleted = i;
                    removed.push(c.1);
                }
            }
            if need_deleted != usize::MAX {
                self.list.remove(need_deleted);
            }

            if self.list.len() < k_bucket {
                self.list.push(cell);
                (true, removed)
            } else {
                if index >= MAX_LEVEL {
                    for v in self.list.iter_mut() {
                        if v > &mut cell {
                            removed.push(v.1);
                            *v = cell;
                            return (true, removed);
                        }
                    }
                    return (false, removed);
                } else {
                    self.right = Some(Box::new(Node::default()));
                    self.left = Some(Box::new(Node::default()));

                    while !self.list.is_empty() {
                        let new_cell = self.list.remove(0);
                        self.insert(new_cell, index, k_bucket);
                    }

                    self.insert(cell, index, k_bucket)
                }
            }
        }
    }

    pub fn search(&self, key: &K, distance: &Distance, index: usize) -> Option<(&K, &u32, bool)> {
        let mut closest_index = usize::MAX;
        let mut closest_distance = Distance::max(K::KEY_LENGTH);

        for (index, cell) in self.list.iter().enumerate() {
            if &cell.0 == key {
                return Some((&cell.0, &cell.1, true));
            } else {
                let dis = distance.xor(&cell.2, K::KEY_LENGTH);
                if dis < closest_distance {
                    closest_distance = dis;
                    closest_index = index;
                }
            }
        }

        if distance.get(index) {
            if let Some(ref right) = self.right {
                let next = right.search(key, distance, index + 1);
                if next.is_some() {
                    return next;
                }
            }
        } else {
            if let Some(ref left) = self.left {
                let next = left.search(key, distance, index + 1);
                if next.is_some() {
                    return next;
                }
            }
        }

        self.list
            .get(closest_index)
            .and_then(|cell| Some((&cell.0, &cell.1, false)))
    }

    pub fn remove(&mut self, key: &K, distance: &Distance, index: usize) -> Option<u32> {
        let mut deleted_index = usize::MAX;
        for (i, cell) in self.list.iter().enumerate() {
            if &cell.0 == key {
                deleted_index = i;
            }
        }

        if deleted_index != usize::MAX {
            let Cell(_k, v, _d) = self.list.remove(deleted_index);
            return Some(v);
        }

        if distance.get(index) {
            if let Some(ref mut right) = self.right {
                return right.remove(key, distance, index + 1);
            }
        } else {
            if let Some(ref mut left) = self.left {
                return left.remove(key, distance, index + 1);
            }
        }

        None
    }

    pub fn keys(&self, vec: &mut Vec<K>) {
        for i in self.list.iter() {
            vec.push(i.key().clone());
        }

        if let Some(ref left) = self.left {
            left.keys(vec);
        }

        if let Some(ref right) = self.right {
            right.keys(vec);
        }
    }

    pub fn is_empty(&self) -> bool {
        if !self.list.is_empty() {
            return false;
        }

        if let Some(ref left) = self.left {
            if !left.is_empty() {
                return false;
            }
        }

        if let Some(ref right) = self.right {
            if !right.is_empty() {
                return false;
            }
        }

        true
    }
}

impl<K: Key> Ord for Cell<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.2.cmp(&other.2)
    }
}

impl<K: Key> PartialOrd for Cell<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K: Key> Eq for Cell<K> {}

impl<K: Key> PartialEq for Cell<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K: Key> Cell<K> {
    fn key(&self) -> &K {
        &self.0
    }
}

#[derive(Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct Distance(BitVec);

impl Distance {
    fn max(len: usize) -> Self {
        Distance(BitVec::from_elem(len, true))
    }

    fn min(len: usize) -> Self {
        Distance(BitVec::from_elem(len, false))
    }

    fn get(&self, index: usize) -> bool {
        if index >= 160 {
            false
        } else {
            self.0[index]
        }
    }

    fn xor(&self, other: &Distance, len: usize) -> Distance {
        let mut new_binary = BitVec::from_elem(len, false);

        for i in 0..len {
            if self.0[i] != other.0[i] {
                new_binary.set(i, true);
            } else {
                new_binary.set(i, false);
            }
        }

        Distance(new_binary)
    }
}

impl Default for Distance {
    fn default() -> Self {
        Distance::min(256)
    }
}
