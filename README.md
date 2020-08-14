[![crate](https://img.shields.io/badge/crates.io-v0.1.1-green.svg)](https://crates.io/crates/chamomile) [![doc](https://img.shields.io/badge/docs.rs-v0.1.1-blue.svg)](https://docs.rs/chamomile)

# Chamomile
*Another P2P Library*

## Example
- `cargo run --example test 0.0.0.0:8000`
- `cargo run --example test 0.0.0.0:8001 0.0.0.0:8000`
- `cargo run --example test 0.0.0.0:8002 0.0.0.0:8000`

## features
- DHT-based & Relay connection.
- Diff transports: UDP/TCP/UDP-Based Special Protocol.

## Design point
- Mobile phones, IoT devices, PC and servers are first-class citizens
- Ability to adapt to the transmission and penetration of complex network environments
- Support for springboard function, virtual connection with other nodes, build virtual DHT
- Encrypted transmission and secure DHT protection
- It can support all interconnections and single-center connections under the LAN, and can also support DHT in the public network environment.
- Automatically switch the connection according to the number of connections and the network environment
- Compatible with part of libp2p and Protocol Lab

## License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.
