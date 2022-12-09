use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::io::Result;
use tokio::sync::mpsc::{Receiver, Sender};

#[inline]
pub fn new_io_error(s: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, s)
}

/// peer's network id.
#[derive(Copy, Clone, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct PeerId(pub [u8; 20]);

pub const PEER_ID_LENGTH: usize = 20;

impl PeerId {
    pub fn short_show(&self) -> String {
        let s = self.to_hex();
        let mut new_hex = String::new();
        new_hex.push_str(&s[0..6]);
        new_hex.push_str("...");
        new_hex.push_str(&s[s.len() - 5..]);
        new_hex
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != PEER_ID_LENGTH {
            return Err(new_io_error("peer id bytes failure."));
        }
        let mut raw = [0u8; PEER_ID_LENGTH];
        raw.copy_from_slice(bytes);
        Ok(Self(raw))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    pub fn from_hex(s: &str) -> Result<PeerId> {
        let raw = if s.starts_with("0x") { &s[2..] } else { s };
        let bytes = hex::decode(raw).map_err(|_| new_io_error("Invalid hex string"))?;
        if bytes.len() != PEER_ID_LENGTH {
            return Err(new_io_error("Invalid address length"));
        }
        let mut fixed_bytes = [0u8; PEER_ID_LENGTH];
        fixed_bytes.copy_from_slice(&bytes);
        Ok(PeerId(fixed_bytes))
    }

    pub fn to_hex(&self) -> String {
        // with checksum encode
        let hex = hex::encode(self.0);

        let mut hasher = Keccak256::new();
        hasher.update(hex.as_bytes());
        let hash = hasher.finalize();
        let check_hash = hex::encode(&hash);

        let mut res = String::from("0x");
        for (index, byte) in hex[..PEER_ID_LENGTH * 2].chars().enumerate() {
            if check_hash.chars().nth(index).unwrap().to_digit(16).unwrap() > 7 {
                res += &byte.to_uppercase().to_string();
            } else {
                res += &byte.to_string();
            }
        }
        res
    }
}

impl Debug for PeerId {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "{}", self.to_hex())
    }
}

/// support some common broadcast algorithm.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Broadcast {
    Gossip,
    StableAll,
}

/// Transports types support by Endpoint.
#[derive(Debug, Copy, Clone, Hash, Deserialize, Serialize, Eq, PartialEq)]
pub enum TransportType {
    QUIC, // 0u8
    TCP,  // 1u8
    RTP,  // 2u8
    UDT,  // 3u8
}

impl TransportType {
    /// transports from parse from str.
    pub fn from_str(s: &str) -> Self {
        match s {
            "quic" => TransportType::QUIC,
            "tcp" => TransportType::TCP,
            "rtp" => TransportType::RTP,
            "udt" => TransportType::UDT,
            _ => TransportType::QUIC,
        }
    }

    pub fn to_str<'a>(&self) -> &'a str {
        match self {
            TransportType::QUIC => "quic",
            TransportType::TCP => "tcp",
            TransportType::RTP => "rtp",
            TransportType::UDT => "udt",
        }
    }

    pub fn from_byte(b: u8) -> Result<Self> {
        match b {
            0u8 => Ok(TransportType::QUIC),
            1u8 => Ok(TransportType::TCP),
            2u8 => Ok(TransportType::RTP),
            3u8 => Ok(TransportType::UDT),
            _ => Err(new_io_error("transport bytes failure.")),
        }
    }

    pub fn to_byte(&self) -> u8 {
        match self {
            TransportType::QUIC => 0u8,
            TransportType::TCP => 1u8,
            TransportType::RTP => 2u8,
            TransportType::UDT => 3u8,
        }
    }
}

#[derive(Debug)]
pub struct TransportStream {
    transport: TransportType,
    sender: Sender<Vec<u8>>,
    receiver: Receiver<Vec<u8>>,
}

impl Eq for TransportStream {}

impl PartialEq for TransportStream {
    fn eq(&self, other: &TransportStream) -> bool {
        self.transport == other.transport
    }
}

impl TransportStream {
    pub fn new(
        transport: TransportType,
        sender: Sender<Vec<u8>>,
        receiver: Receiver<Vec<u8>>,
    ) -> Self {
        Self {
            transport,
            sender,
            receiver,
        }
    }

    pub fn channel(self) -> (Sender<Vec<u8>>, Receiver<Vec<u8>>) {
        (self.sender, self.receiver)
    }
}
