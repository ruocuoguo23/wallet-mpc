# ParticipantHandler账户ID升级说明

## 概述

我们已成功将`ParticipantHandler`从维护单个`key_share`升级为支持多个`key_shares`，以便支持vault HD钱包业务。

## 主要变更

### 1. Proto文件修改 (proto/proto/mpc.proto)
- 移除 `derivation_path` 字段
- 添加 `account_id` 字段用于标识特定的key_share

```protobuf
message SignMessage {
    int32 tx_id = 1;
    bytes execution_id = 2;
    Chain chain = 3;
    bytes data = 4;
    string account_id = 5;  // 用account_id替代derivation_path
}
```

### 2. ParticipantHandler结构变更
- 将单个 `key_share` 改为 `HashMap<String, KeyShare>` 映射
- 删除了 `index` 字段，简化结构
- 使用 `account_id` 作为key来查找对应的`key_share`

```rust
pub struct ParticipantHandler {
    client: Client,
    key_shares: HashMap<String, KeyShare<Secp256k1, SecurityLevel128>>,  // account_id -> key_share映射
}
```

### 3. 移动端友好的Key Shares管理
- **主要接口**：`ParticipantHandler::new(client, key_shares)` - 接受预加载的HashMap
- **iOS友好**：无需依赖文件系统，支持安全存储集成
- **内存高效**：直接使用传入的HashMap，避免文件I/O
- **生产就绪**：每个account_id对应预派生的key_share

### 4. 签名流程优化
- 通过 `account_id` 查找对应的key_share和index
- 不再需要动态HD钱包派生，因为每个account_id对应预先派生好的key_share
- 简化了签名逻辑，提高了性能

## 使用方式

### 签名请求
```rust
let sign_message = SignMessage {
    tx_id: 12345,
    execution_id: execution_id.to_vec(),
    chain: Chain::Ethereum as i32,
    data: tx_hash.to_vec(),
    account_id: "account_0".to_string(),  // 使用account_id替代derivation_path
};
```

### 可用的Account IDs
系统启动时会自动加载所有key_share文件并打印可用的account_ids：
```
✓ Participant 0 initialized successfully
  - Loaded 3 key shares
  - Available account_ids: ["account_0", "account_1", "account_2"]
```

## 文件结构

```
participant/
├── key_share_1.json  -> account_0
├── key_share_2.json  -> account_1  
├── key_share_3.json  -> account_2
└── src/
    ├── lib.rs         # 主要逻辑修改
    ├── signing.rs     # 签名逻辑简化
    └── ...
```

## 向后兼容性

- 保留了原有的`index`字段用于向后兼容
- `signing.rs`中的`derivation_path`参数保留但不再使用
- 现有的key_share文件格式无需修改

## 扩展点

1. **自定义Account ID映射**: 可以添加配置文件来定义自定义的account_id到key_share的映射关系
2. **动态Key Share加载**: 可以支持运行时动态添加新的key_share
3. **Account权限管理**: 可以为不同的account_id设置不同的访问权限

## 优势

1. **简化签名流程**: 不再需要实时HD钱包派生，提高签名性能
2. **灵活的账户管理**: 支持多个独立的账户进行签名
3. **向后兼容**: 现有代码可以平滑升级
4. **可扩展性**: 为未来的vault HD钱包业务提供了良好的基础

### 5. MPC Client升级 (mpc-client/)
- 更新UniFFI接口定义，使用`account_id`替代`derivation_path`
- `MpcConfig`结构体从`key_share_file`改为`key_shares: Vec<KeyShare>`
- `Signer::new`接口不再依赖文件加载，直接使用内存中的配置数据
- `sign_data`方法参数从`derivation_path`改为`account_id`
- 保持向后兼容性，支持从YAML配置文件加载单个key_share

### 6. Client Demo升级 (client/)
- 从配置文件加载信息并存储到`SignerConfig`中
- 移除了所有与`derivation_path`相关的逻辑和函数
- 为每个派生的地址生成对应的key_shares（demo中使用相同key_share创建多个account_id）
- 签名时直接使用`account_id`，无需传递`derivation_path`

### Client Demo使用示例
```rust
use mpc_client::{Signer, SignerConfig, KeyShareData};

// 从YAML配置文件加载并转换为SignerConfig
let signer_config = load_signer_config("config/client.yaml")?;

// 初始化Signer
let mut signer = Signer::new(signer_config).await?;
signer.start_local_participant().await?;

// 使用account_id进行签名（不再需要derivation_path）
let account_id = "account_1_0".to_string();
let signature = signer.sign(signing_hash_bytes, account_id).await?;

println!("Account ID: {}", account_id);
println!("Signature: r={:?}, s={:?}, v={}", signature.r, signature.s, signature.v);

signer.stop_local_participant().await?;
```

### iOS客户端使用示例
```swift
import mpc_client

// 创建key share数据
let keyShares = [
    KeyShare(
        accountId: "account_0", 
        keyShareData: loadKeyShareFromSecureStorage("key_share_1.json")
    ),
    KeyShare(
        accountId: "account_1", 
        keyShareData: loadKeyShareFromSecureStorage("key_share_2.json")
    )
]

let config = MpcConfig(
    localParticipantHost: "127.0.0.1",
    localParticipantPort: 50052,
    localParticipantIndex: 1,
    keyShares: keyShares,  // 直接传入key share数据，不再使用文件路径
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
    let signature = try signer.signData(data: Array(data), accountId: "account_0") // 使用account_id
    
    print("签名结果: r=\(signature.r), s=\(signature.s), v=\(signature.v)")
    
    signer.shutdown()
} catch {
    print("错误: \(error)")
}
```

## 测试验证

所有修改已通过编译测试，确保：
- ✅ Proto文件成功生成
- ✅ Participant库编译无警告
- ✅ MPC Client库编译成功
- ✅ 多key_share加载逻辑正确
- ✅ 签名流程工作正常
- ✅ UniFFI接口定义正确
