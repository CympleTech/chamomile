## Chamomile
*Another P2P implement*

### features cfg
- serializtion abstrat: capnproto/protobuf/flatbuffer
- transport abstrat: UDP/QUIC/TCP
- RPC abstrat: gRPC/capnprotoRPC/JSONRPC

### inner public key
- Keys abstrat: RSA/Ed25519/Secp256k1/ECDSA

## Design point
- Compatible with part of libp2p and Protocol Lab
- Provide multiple types of features settings
- Mobile phones and IoT devices and PC, servers are first-class citizens
- Ability to adapt to the transmission and penetration of complex network environments
- Support for springboard function, virtual connection with other nodes, build virtual DHT
- Encrypted transmission and secure DHT protection
- It can support all interconnections and single-center connections under the LAN, and can also support DHT in the public network environment.
- Automatically switch the connection according to the number of connections and the network environment
