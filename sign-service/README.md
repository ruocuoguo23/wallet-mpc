# Sign Service

Sign Service是一个综合性的MPC签名服务，它同时运行SSE服务器和Participant服务器，为多方计算签名提供完整的基础设施。

## 功能特性

- **双服务器架构**: 同时运行SSE服务器(消息传递)和Participant服务器(MPC签名)
- **YAML配置**: 使用配置文件管理所有设置
- **自动Key Share管理**: 根据participant index自动加载对应的密钥分片
- **完整的日志记录**: 支持多种日志级别和格式
- **优雅的错误处理**: 提供详细的错误信息和恢复机制

## 配置文件

配置文件位于 `config/sign-service.yaml`:

```yaml
server:
  sse:
    host: "127.0.0.1"
    port: 8080
    cors_origins: ["http://localhost:3000", "http://127.0.0.1:3000"]
  
  participant:
    host: "127.0.0.1"
    port: 50051
    index: 0  # 参与者索引 (0, 1, 或 2)

logging:
  level: "info"
  format: "json"

mpc:
  threshold: 2
  total_participants: 3
  key_share_file: "participant/key_share_1.json"
```

## 使用方法

### 编译

```bash
cargo build -p sign-service
```

### 运行

使用默认配置文件:
```bash
cargo run -p sign-service
```

指定配置文件:
```bash
cargo run -p sign-service -- /path/to/your/config.yaml
```

### 测试

```bash
cargo test -p sign-service
```

## 服务端点

启动后，Sign Service会提供以下服务：

### SSE服务器 (默认端口 8080)
- `GET /rooms/{room_id}/subscribe` - 订阅消息
- `POST /rooms/{room_id}/issue_unique_idx` - 获取唯一索引
- `POST /rooms/{room_id}/broadcast` - 广播消息

### Participant服务器 (默认端口 50051)
- gRPC服务，提供 `sign_tx` 方法用于交易签名

## Key Share文件

服务会根据participant index自动加载对应的key share文件：
- index 0 → `participant/key_share_1.json`
- index 1 → `participant/key_share_2.json`  
- index 2 → `participant/key_share_3.json`

## 日志

支持以下日志级别：
- `error` - 错误信息
- `warn` - 警告信息
- `info` - 一般信息 (默认)
- `debug` - 调试信息
- `trace` - 详细跟踪信息

## 架构说明

Sign Service作为MPC签名基础设施的核心组件：

1. **SSE服务器**: 负责参与者之间的消息传递和同步
2. **Participant服务器**: 处理实际的MPC签名请求
3. **配置管理**: 统一管理所有服务配置
4. **安全性**: 每个参与者只能访问自己的密钥分片

这为后续与客户端的集成和以太坊交易的多方签名奠定了坚实的基础。
