[![crate](https://img.shields.io/badge/crates.io-v0.10.7-green.svg)](https://crates.io/crates/chamomile) [![doc](https://img.shields.io/badge/docs.rs-v0.10.7-blue.svg)](https://docs.rs/chamomile)

# Chamomile
*Build a robust stable connection on p2p network*

## features
- Support build a robust stable connection between two peers on the p2p network.
- Support permissionless network.
- Support permissioned network (distributed network).
- DHT-based & Relay connection.
- Diff transports: QUIC(*default*) / TCP / UDP-Based Special Protocol.
- Multiple transports connecting at same runtime.

## Simple test.
- A: `cargo run --example permissionless 127.0.0.1:8000`
- B: `cargo run --example permissionless 127.0.0.1:8001 127.0.0.1:8000`
- C: `cargo run --example permissionless 127.0.0.1:8002 127.0.0.1:8000`

If not support `127.0.0.1` binding, you can change to `0.0.0.0` and try again.

## Relay test.
- A: `cargo run --example relay 192.168.xx.xx:8000`
  - this ip is your LAN address, it will do relay work.
- B: `cargo run --example relay 127.0.0.1:8001 192.168.xx.xx:8000`
  - start waiting stable connected by relay.
- C: `cargo run --example relay 127.0.0.1:8002 192.168.xx.xx:8000 XX..`
  - XX.. is above's B network `peer id` will connected it.
  - And if change B and C `127.0.0.1` to `0.0.0.0`, they will automatically connect after the handshake is successful, no longer need relay.

## Design point
- Mobile phones, IoT devices, PC and servers are first-class citizens
- Ability to adapt to the transmission and penetration of complex network environments
- Support for springboard function, virtual connection with other nodes, build virtual DHT
- Encrypted transmission and secure DHT protection
- It can support all interconnections and single-center connections under the LAN, and can also support DHT in the public network environment
- Automatically switch the connection according to the number of connections and the network environment
- If Alice use QUIC, Bob use TCP, they can still connect and communicate with each other.

## For more information, please visit:
- Website: https://cympletech.com
- Twitter: https://twitter.com/cympletech
- Discord: https://discord.gg/UfFjp6Kaj4
- E-mail: dev@cympletech.com

## License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)
 * Anti-War license ([LICENSE-AW](LICENSE-AW) or
   https://github.com/sunhuachuang/AW-License)

at your option.
