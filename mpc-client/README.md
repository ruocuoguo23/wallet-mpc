# MPC Client Library

MPC Client Library 是一个用于iOS和其他平台的MPC钱包库，通过UniFFI提供跨平台接口。

## 功能特性

- **UniFFI绑定**: 支持iOS Swift、Android Kotlin等多平台调用
- **MPC签名**: 提供3-2门限签名能力
- **配置灵活**: 支持动态配置participant参数
- **异步支持**: 基于Tokio的异步运行时

## 编译

### iOS静态库

```bash
# 编译iOS目标
cargo build --target aarch64-apple-ios --release
cargo build --target aarch64-apple-ios-sim --release

# 生成Swift绑定
cargo run --features=uniffi/cli --bin uniffi-bindgen \
    generate \
    --library target/release/libmpc_client.dylib \
    --language swift \
    --out-dir bindings/ios
```

### 使用release脚本

```bash
# 编译所有iOS目标并生成绑定
./release.sh --targets aarch64-apple-ios aarch64-apple-ios-sim --release
```

## 使用方法

### Swift (iOS)

```swift
import mpc_client

let config = MpcConfig(
    localParticipantHost: "127.0.0.1",
    localParticipantPort: 50052,
    localParticipantIndex: 1,
    keyShareFile: "key_share_2.json",
    signServiceHost: "127.0.0.1", 
    signServicePort: 50051,
    sseHost: "127.0.0.1",
    ssePort: 8080,
    signServiceIndex: 0,
    threshold: 2,
    totalParticipants: 3,
    logLevel: "info"
)

do {
    let signer = try MpcSigner(config: config)
    try signer.initialize()
    
    let data = Data([1, 2, 3, 4]) // 要签名的数据
    let signature = try signer.signData(data: Array(data), derivationPath: nil)
    
    print("签名结果: r=\(signature.r), s=\(signature.s), v=\(signature.v)")
    
    signer.shutdown()
} catch {
    print("错误: \(error)")
}
```

## API接口

### MpcSigner

- `MpcSigner(config: MpcConfig)` - 创建MPC签名器实例
- `initialize()` - 初始化签名器，启动本地participant
- `signData(data: [UInt8], derivationPath: [UInt32]?)` - 对数据进行MPC签名
- `shutdown()` - 关闭签名器，清理资源

### MpcConfig

配置结构体，包含所有MPC参数：
- `localParticipantHost/Port/Index` - 本地participant配置
- `signServiceHost/Port/Index` - 远程sign-service配置  
- `sseHost/Port` - SSE服务器配置
- `keyShareFile` - 密钥分片文件路径
- `threshold/totalParticipants` - 门限签名参数
- `logLevel` - 日志级别

## 项目结构

```
mpc-client/
├── src/
│   ├── lib.rs              # 主库文件，UniFFI接口
│   ├── signer.rs           # MPC签名器实现
│   └── mpc_client.udl      # UniFFI接口定义
├── build.rs                # 编译脚本
├── Cargo.toml              # 项目配置
└── README.md               # 本文档
