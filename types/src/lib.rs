pub mod key;
pub mod message;
pub mod peer;
pub mod types;

pub use peer::Peer;
pub use types::PeerId;

/// delivery data.
#[macro_export]
macro_rules! delivery_split {
    ($data:expr, $length:expr) => {
        if $length == 0 {
            Vec::new()
        } else if $data.len() < $length {
            $data.clone()
        } else {
            $data[0..$length].to_vec()
        }
    };
}
