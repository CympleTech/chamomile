pub mod message;
pub mod types;

/// delivery data.
#[macro_export]
macro_rules! delivery_split {
    ($data:expr, $length:expr) => {
        $data[0..core::cmp::min($data.len(), $length)].to_vec()
    };
}
