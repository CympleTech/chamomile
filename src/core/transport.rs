use serde_derive::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, Hash, Deserialize, Serialize)]
pub enum Transport {
    UDP(SocketAddr, bool), // 0u8
    TCP(SocketAddr, bool), // 1u8
    RTP(SocketAddr, bool), // 2u8
    UDT(SocketAddr, bool), // 3u8
}

impl Transport {
    fn symbol(&self) -> u8 {
        match self {
            &Transport::UDP(_, _) => 0u8,
            &Transport::TCP(_, _) => 1u8,
            &Transport::RTP(_, _) => 2u8,
            &Transport::UDT(_, _) => 3u8,
        }
    }

    fn is_public(&self) -> bool {
        match self {
            &Transport::UDP(_, is_public)
            | &Transport::TCP(_, is_public)
            | &Transport::RTP(_, is_public)
            | &Transport::UDT(_, is_public) => is_public,
        }
    }

    pub fn addr(&self) -> &SocketAddr {
        match self {
            &Transport::UDP(ref addr, _)
            | &Transport::TCP(ref addr, _)
            | &Transport::RTP(ref addr, _)
            | &Transport::UDT(ref addr, _) => addr,
        }
    }
}
