## Libp2p

### features
- serializtion abstrat: capnproto/protobuf/flatbuffer
- transport abstrat: UDP/QUIC/TCP
- RPC abstrat: gRPC/capnprotoRPC/JSONRPC

### Enums
- Keys abstrat: RSA/Ed25519/Secp256k1/ECDSA

## 设计点
1. 兼容libp2p
2. 提供多种类型的features设定
3. 手机和物联网设备与PC, 服务器均作为一等公民
4. 能够适应各种复杂的网络环境的传输和穿透
5. 既能在局域网中运行, 也可以在公网环境下完好运行
6. 支持跳板功能, 如果和其他局域网下的设备无法完成穿透, 则借助公网环境做虚拟连接
7. 加密传输和校验
8. 安全的DHT表

