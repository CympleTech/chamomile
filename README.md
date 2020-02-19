## Chamomile
*Another P2P Library*

### features
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

## Example
- `cargo run --example test 0.0.0.0:8000`
- `cargo run --example test 0.0.0.0:8001 0.0.0.0:8000`
- `cargo run --example test 0.0.0.0:8002 0.0.0.0:8000`
