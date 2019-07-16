mod core;
mod keys;
mod protocol;
mod secio;
mod serialization;
mod transports;

pub mod prelude {
    pub use super::core::peer_id::PeerID;
    pub use super::serialization::SerializationType;
    pub use super::transports::TransportType;
}

pub mod actor {
    pub use super::core::message::*;
    pub use super::core::server::ServerActor;

    pub mod actix {
        pub use actix::prelude::*;
    }
}

pub mod tokio {
    // channel
}
