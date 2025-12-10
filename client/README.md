# MPC Wallet Client

MPC Wallet Client 是一个基于多方计算(MPC)的以太坊钱包客户端，它运行本地的participant server并与远程sign-gateway协作完成2-2门限签名。

## 功能特性

- **本地Participant服务器**: 运行自己的MPC participant，保护本地密钥分片
- **远程协作**: 与sign-gateway和其他participants协作完成签名
- **以太坊集成**: 支持以太坊交易的构造、签名和发送
- **2-2门限签名**: 2个participants都需要参与才能完成签名
- **YAML配置**: 灵活的配置文件管理
- **完整日志**: 详细的操作日志和错误追踪

## 项目结构

```
client/
├── src/
│   ├── main.rs       # 主程序入口，演示完整的签名流程
│   └── signer.rs     # Signer实现，核心MPC签名逻辑
├── sample/           # 样例代码参考
└── README.md         # 本文档
```

## 配置文件

配置文件位于 `config/client.yaml`:

```yaml
# 本地participant配置
local_participant:
  host: "127.0.0.1"
  port: 50052
  index: 1     # 本客户端作为participant 1
  key_share_file: "participant/key_share_2.json"

# 远程sign-gateway配置
remote_services:
  sign_gateway:
    participant_host: "127.0.0.1"
    participant_port: 50050

# Ethereum provider配置
ethereum:
  provider_url: "https://eth-sepolia.g.alchemy.com/v2/your-api-key"
  chain_id: 11155111

# MPC配置
mpc:
  threshold: 2
  total_participants: 2

# 日志配置
logging:
  level: "info"
  format: "text"
```

## 使用方法

### 编译

```bash
cargo build -p client
```

### 运行

确保sign-gateway已经启动，然后运行客户端:

```bash
cargo run -p client
```

指定配置文件:
```bash
cargo run -p client -- /path/to/your/config.yaml
```

### 运行示例

1. 启动sign-gateway:
```bash
cargo run -p sign-gateway
```

2. 启动sign-service:
```bash
cargo run -p sign-service
```

3. 在新终端中启动client:
```bash
cargo run -p client
```

## 核心组件

### Signer

`Signer` 是核心组件，提供以下功能：

- `new(config_path)` - 初始化Signer，连接到远程服务
- `start_local_participant()` - 启动本地participant server
- `sign_transaction()` - 对以太坊交易进行MPC签名
- `sign_and_send_transaction()` - 签名并发送交易到区块链
- `stop_local_participant()` - 停止本地服务

### 使用示例代码

```rust
use client::signer::Signer;
use alloy::primitives::{Address, U256};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化Signer
    let mut signer = Signer::new("config/client.yaml").await?;
    
    // 启动本地participant
    signer.start_local_participant().await?;
    
    // 构造交易
    let to: Address = "0x742d35Cc6663C88564Cdf5D127d0C1B3B4D0a7b2".parse()?;
    let value = U256::from(1000000000000000000u64); // 1 ETH
    let data = Vec::new();
    
    // 执行MPC签名并发送交易
    let tx_hash = signer.sign_and_send_transaction(to, value, data).await?;
    println!("Transaction hash: {}", tx_hash);
    
    // 清理
    signer.stop_local_participant().await?;
    
    Ok(())
}
```

## MPC签名流程

1. **初始化**: 加载配置，连接远程服务，创建以太坊provider
2. **启动本地Participant**: 在指定端口启动gRPC服务器
3. **构造交易**: 创建以太坊原始交易并RLP编码
4. **MPC签名**: 
   - 生成唯一执行ID
   - 向远程participants发送签名请求
   - 收集并验证签名响应
   - 合并得到最终签名
5. **发送交易**: 构造已签名交易并广播到网络

## 安全性

- **密钥分片隔离**: 每个participant只能访问自己的密钥分片
- **门限安全**: 需要至少2个participants参与才能完成签名
- **网络安全**: 支持TLS加密通信 (可配置)
- **执行隔离**: 每次签名使用唯一的执行ID防止重放

## 网络架构

```
┌─────────────────┐         ┌──────────────────┐
│   Client        │◄────────┤  Sign-Gateway    │
│  (Local         │  gRPC   │  (Proxy to      │
│   Participant 1)│         │   Sign-Service) │
└─────────────────┘         └──────────────────┘
         │                           
         │         SSE Messages      
         └───────────┬─────────────────┘
                     │
            ┌─────────▼──────────┐
            │    Participant 2   │
            │   (Optional 3rd)   │
            └────────────────────┘
```

## 故障排查

### 常见问题

1. **连接失败**: 检查sign-gateway是否正在运行
2. **端口冲突**: 修改配置文件中的端口号
3. **密钥分片错误**: 确认key_share文件路径和内容正确
4. **签名失败**: 检查SSE服务器连接和participant通信

### 日志级别

- `error`: 错误和异常
- `warn`: 警告信息
- `info`: 一般操作信息 (推荐)
- `debug`: 详细调试信息
- `trace`: 最详细的跟踪信息

## 测试

运行单元测试:
```bash
cargo test -p client
```

运行集成测试 (需要启动sign-service):
```bash
cargo test -p client -- --ignored
```

## 开发

### 添加新的链支持

1. 在 `proto/mpc.proto` 中添加新的Chain类型
2. 在 `signer.rs` 中实现对应的交易构造逻辑
3. 更新配置文件支持新链的参数

### 自定义签名逻辑

继承或修改 `Signer` 结构体，重写相应的方法来实现自定义逻辑。

## 路线图

- [ ] 支持多链 (BSC, Polygon等)
- [ ] 动态participant发现
- [ ] 交易批处理支持
- [ ] Web界面集成
- [ ] 硬件安全模块(HSM)集成
