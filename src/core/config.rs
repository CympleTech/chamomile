use multiaddr::Multiaddr;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;

use crate::core::primitives::DEFAULT_TRANSPORT_SOCKET;
use crate::transports::TransportType;

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigureRow {
    p2p_listen: String,
    p2p_default_listens: Vec<String>,
    p2p_bootstraps: Vec<String>,
}

impl ConfigureRow {
    fn parse(&self) -> Configure {
        let main_multiaddr: Multiaddr = self.p2p_listen.parse().expect("Not Have P2P Listen");
        let main_transport = TransportType::from_multiaddr(&main_multiaddr);

        let mut transports_socket = DEFAULT_TRANSPORT_SOCKET.clone();
        transports_socket.insert(main_transport.clone(), main_multiaddr);
        let mut default_transports_socket: Vec<(TransportType, Multiaddr)> = self
            .p2p_default_listens
            .iter()
            .filter_map(|p| {
                p.parse::<Multiaddr>()
                    .map(|maddr| (TransportType::from_multiaddr(&maddr), maddr))
                    .ok()
            })
            .collect();

        while !default_transports_socket.is_empty() {
            let (t, m) = default_transports_socket.pop().unwrap();
            transports_socket.insert(t, m);
        }

        let bootstraps = self
            .p2p_bootstraps
            .iter()
            .filter_map(|p| p.parse().ok())
            .collect();

        Configure::new(main_transport, transports_socket, bootstraps)
    }
}

pub struct Configure {
    pub main_transport: TransportType,
    pub transports_socket: HashMap<TransportType, Multiaddr>,
    pub bootstraps: Vec<Multiaddr>,
}

impl Configure {
    fn new(
        main_transport: TransportType,
        transports_socket: HashMap<TransportType, Multiaddr>,
        bootstraps: Vec<Multiaddr>,
    ) -> Self {
        Configure {
            main_transport,
            transports_socket,
            bootstraps,
        }
    }

    fn default() -> Self {
        let main_transport = TransportType::default();
        let transports_socket = DEFAULT_TRANSPORT_SOCKET.clone();
        let bootstraps = vec![];

        Configure::new(main_transport, transports_socket, bootstraps)
    }

    pub fn load() -> Self {
        let string = load_file_string();
        if string.is_none() {
            return Configure::default();
        }

        let config_row: ConfigureRow = toml::from_str(&string.unwrap()).unwrap();
        config_row.parse()
    }

    pub fn main_multiaddr(&self) -> &Multiaddr {
        self.transports_socket.get(&self.main_transport).unwrap()
    }
}

fn load_file_string() -> Option<String> {
    let file_path = "config.toml";
    let mut file = match File::open(file_path) {
        Ok(f) => f,
        Err(_) => {
            return None;
        }
    };

    let mut str_val = String::new();
    match file.read_to_string(&mut str_val) {
        Ok(s) => s,
        Err(e) => panic!("Error Reading file: {}", e),
    };
    Some(str_val)
}
