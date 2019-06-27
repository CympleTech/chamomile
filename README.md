## Chamomile

### features cfg
- serializtion abstrat: capnproto/protobuf/flatbuffer
- transport abstrat: UDP/QUIC/TCP
- RPC abstrat: gRPC/capnprotoRPC/JSONRPC

### inner public key
- Keys abstrat: RSA/Ed25519/Secp256k1/ECDSA

## 设计点
1. 兼容libp2p
2. 提供多种类型的features设置
3. 手机和物联网设备与PC, 服务器均作为一等公民
4. 能够适应各种复杂的网络环境的传输和穿透
6. 支持跳板功能, 借助其他节点做虚拟连接,构建虚拟DHT
7. 加密传输和安全DHT保护
8. 既能支持局域网下全体互联和单中心连接,也能支持公网环境下的DHT
9. 根据连接数量和网络环境自动进行连接形式的切换
