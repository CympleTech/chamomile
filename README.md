[![crate](https://img.shields.io/badge/crates.io-v0.6.2-green.svg)](https://crates.io/crates/chamomile) [![doc](https://img.shields.io/badge/docs.rs-v0.6.2-blue.svg)](https://docs.rs/chamomile)

# Chamomile
*Build a robust stable connection on p2p network*

## features
- Support build a robust stable connection between two peers on the p2p network.
- Support permissionless network.
- Support permissioned network (distributed network).
- DHT-based & Relay connection.
- Diff transports: QUIC(*default*) / TCP / UDP-Based Special Protocol.

## Simple test.
- A: `cargo run --example permissionless 0.0.0.0:8000`
- B: `cargo run --example permissionless 0.0.0.0:8001 0.0.0.0:8000`
- C: `cargo run --example permissionless 0.0.0.0:8002 0.0.0.0:8000`

If not support `0.0.0.0` binding, you can change to `127.0.0.1`.

## Relay test.
- A: `cargo run --example relay 192.168.xx.xx:8000`
  - this ip is your LAN address, it will do relay work.
- B: `cargo run --example relay 127.0.0.1:8001 192.168.xx.xx:8000`
  - start waiting stable connected by relay.
- C: `cargo run --example relay 127.0.0.1:8002 192.168.xx.xx:8000 XX..`
  - XX.. is above's B network `peer id` will connected it.

## Design point
- Mobile phones, IoT devices, PC and servers are first-class citizens
- Ability to adapt to the transmission and penetration of complex network environments
- Support for springboard function, virtual connection with other nodes, build virtual DHT
- Encrypted transmission and secure DHT protection
- It can support all interconnections and single-center connections under the LAN, and can also support DHT in the public network environment.
- Automatically switch the connection according to the number of connections and the network environment

## License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.
