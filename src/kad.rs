use bit_vec::BitVec;
use chamomile_types::types::PeerId;
use core::cmp::Ordering;
use smol::channel::Sender;

use crate::peer::Peer;
use crate::session::SessionMessage;
use crate::transports::EndpointMessage;

const MAX_LEVEL: usize = 8;
const K_BUCKET: usize = 8;

pub(crate) struct KadValue(
    pub Sender<SessionMessage>,
    pub Sender<EndpointMessage>,
    pub Peer,
);

pub(crate) struct KadTree {
    root_key: PeerId,
    left: TreeNode,
    right: TreeNode,
}

type TreeNode = Option<Box<Node>>;

struct Node {
    left: TreeNode,
    right: TreeNode,
    list: Vec<Cell>,
}

struct Cell(PeerId, KadValue, Distance);

impl KadTree {
    pub fn new(key: PeerId) -> Self {
        KadTree {
            root_key: key,
            left: None,
            right: None,
        }
    }

    pub fn add(&mut self, key: PeerId, value: KadValue) -> bool {
        let distance = Distance::new(&self.root_key, &key);

        if distance.get(0) {
            if self.right.is_none() {
                self.right = Some(Box::new(Node::default()));
            }
            self.right
                .as_mut()
                .and_then(|v| Some(v.insert(Cell(key, value, distance), 1, K_BUCKET)))
                .unwrap()
        } else {
            if self.left.is_none() {
                self.left = Some(Box::new(Node::default()));
            }
            self.left
                .as_mut()
                .and_then(|v| Some(v.insert(Cell(key, value, distance), 1, K_BUCKET)))
                .unwrap()
        }
    }

    pub fn search(&self, key: &PeerId) -> Option<(&PeerId, &KadValue, bool)> {
        let distance = Distance::new(&self.root_key, &key);

        if distance.get(0) {
            if self.right.is_none() {
                if self.left.is_none() {
                    None
                } else {
                    self.left
                        .as_ref()
                        .and_then(|v| Some(v.search(key, &distance, 1)))
                        .unwrap()
                }
            } else {
                self.right
                    .as_ref()
                    .and_then(|v| Some(v.search(key, &distance, 1)))
                    .unwrap()
            }
        } else {
            if self.left.is_none() {
                if self.right.is_none() {
                    None
                } else {
                    self.right
                        .as_ref()
                        .and_then(|v| Some(v.search(key, &distance, 1)))
                        .unwrap()
                }
            } else {
                self.left
                    .as_ref()
                    .and_then(|v| Some(v.search(key, &distance, 1)))
                    .unwrap()
            }
        }
    }

    pub fn remove(&mut self, key: &PeerId) -> Option<KadValue> {
        let distance = Distance::new(&self.root_key, &key);
        if distance.get(0) {
            self.right
                .as_mut()
                .and_then(|v| v.remove(key, &distance, 1))
        } else {
            self.left.as_mut().and_then(|v| v.remove(key, &distance, 1))
        }
    }

    pub fn contains(&self, key: &PeerId) -> bool {
        if let Some((_, _, true)) = self.search(key) {
            true
        } else {
            false
        }
    }

    pub fn keys(&self) -> Vec<PeerId> {
        let mut vec = Vec::new();
        if self.left.is_some() {
            self.left.as_ref().unwrap().keys(&mut vec);
        }
        if self.right.is_some() {
            self.right.as_ref().unwrap().keys(&mut vec);
        }
        vec
    }
}

impl Node {
    fn default() -> Self {
        Node {
            left: None,
            right: None,
            list: Vec::new(),
        }
    }

    fn insert(&mut self, mut cell: Cell, index: usize, k_bucket: usize) -> bool {
        if self.right.is_some() || self.left.is_some() {
            if cell.2.get(index) {
                if self.right.is_none() {
                    self.right = Some(Box::new(Node::default()));
                }
                self.right
                    .as_mut()
                    .and_then(|v| Some(v.insert(cell, index + 1, k_bucket)))
                    .unwrap()
            } else {
                if self.left.is_none() {
                    self.left = Some(Box::new(Node::default()));
                }
                self.left
                    .as_mut()
                    .and_then(|v| Some(v.insert(cell, index + 1, k_bucket)))
                    .unwrap()
            }
        } else {
            let mut need_deleted = usize::MAX;
            for (i, c) in self.list.iter().enumerate() {
                if c == &cell {
                    need_deleted = i;
                }
            }
            if need_deleted != usize::MAX {
                self.list.remove(need_deleted);
            }

            if self.list.len() < k_bucket {
                self.list.push(cell);
                true
            } else {
                if index >= MAX_LEVEL {
                    for v in self.list.iter_mut() {
                        if v > &mut cell {
                            *v = cell;
                            return true;
                        }
                    }
                    return false;
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

    pub fn search(
        &self,
        key: &PeerId,
        distance: &Distance,
        index: usize,
    ) -> Option<(&PeerId, &KadValue, bool)> {
        let mut closest_index = usize::MAX;
        let mut closest_distance = Distance::max();

        for (index, cell) in self.list.iter().enumerate() {
            if &cell.0 == key {
                return Some((&cell.0, &cell.1, true));
            } else {
                let dis = distance.xor(&cell.2);
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

    pub fn remove(&mut self, key: &PeerId, distance: &Distance, index: usize) -> Option<KadValue> {
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

    pub fn keys(&self, vec: &mut Vec<PeerId>) {
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
}

impl Ord for Cell {
    fn cmp(&self, other: &Cell) -> Ordering {
        self.2.cmp(&other.2)
    }
}

impl PartialOrd for Cell {
    fn partial_cmp(&self, other: &Cell) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Cell {}

impl PartialEq for Cell {
    fn eq(&self, other: &Cell) -> bool {
        self.0 == other.0
    }
}

impl Cell {
    fn key(&self) -> &PeerId {
        &self.0
    }
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
pub struct Distance(BitVec);

impl Distance {
    pub fn max() -> Self {
        Distance(BitVec::from_elem(160, true))
    }

    pub fn min() -> Self {
        Distance(BitVec::from_elem(160, false))
    }

    pub fn new(base: &PeerId, target: &PeerId) -> Self {
        let base_source = BitVec::from_bytes(base.as_bytes());
        let base = Distance((0..160).map(|i| base_source[i]).collect());

        let target_source = BitVec::from_bytes(target.as_bytes());
        let target = Distance((0..160).map(|i| target_source[i]).collect());

        base.xor(&target)
    }

    pub fn get(&self, index: usize) -> bool {
        if index >= 160 {
            false
        } else {
            self.0[index]
        }
    }

    pub fn xor(&self, other: &Distance) -> Distance {
        let mut new_binary = BitVec::from_elem(160, false);

        for i in 0..160 {
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
        Distance::min()
    }
}
