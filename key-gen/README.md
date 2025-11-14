# Key Share Generator for MPC HD Wallet

这是一个为 MPC HD 钱包生成密钥分片的工具。由于 MPC 的 HD 钱包派生机制与传统 BIP-32 不同，此工具接受**已派生的子密钥（child key）**作为输入，直接生成对应的 MPC 密钥分片。

## 核心概念

⚠️ **重要说明**：
- 此工具接受的是**已派生的子密钥（child key）**，而非主密钥（master key）
- 你需要在外部使用 BIP-32 等标准先派生出子密钥
- MPC HD 钱包的派生机制与传统 HD 钱包不同，因此需要为每个账户单独生成 MPC 密钥分片

## 功能特点

- ✅ 从子密钥（child key）生成 threshold MPC 密钥分片（如 2-of-3）
- ✅ 支持多个 account_id 的密钥分片存储
- ✅ 支持追加模式，可以多次运行程序添加新账户
- ✅ 每个参与方有独立的密钥分片文件

## 使用方法

### 基本用法

```bash
cargo run -- \
  --child-key <CHILD_KEY_HEX> \
  --account-id "account_0"
```

### 自定义参数

```bash
cargo run -- \
  --child-key <CHILD_KEY_HEX> \
  --account-id "m/44'/60'/0'/0/0" \
  --n-parties 5 \
  --threshold 3 \
  --output "my_keys"
```

## 命令行参数

| 参数 | 缩写 | 必需 | 描述 |
|------|------|------|------|
| `--child-key` | `-k` | ✅ | 子密钥（64位十六进制，32字节） |
| `--account-id` | `-a` | ✅ | 账户ID（任意字符串，建议使用派生路径） |
| `--n-parties` | `-n` | ❌ | 参与方数量（默认: 3） |
| `--threshold` | `-t` | ❌ | 签名阈值（默认: 2） |
| `--output` | `-o` | ❌ | 输出文件前缀（默认: "key_shares"） |

## 输出文件

程序会生成 N 个文件（N = 参与方数量）：

- `key_shares_1.json` - 参与方 1 的密钥分片
- `key_shares_2.json` - 参与方 2 的密钥分片
- `key_shares_3.json` - 参与方 3 的密钥分片

每个文件支持存储多个账户的密钥分片，格式如下：

```json
{
  "account_0": { /* 密钥分片数据 */ },
  "m/44'/60'/0'/0/1": { /* 密钥分片数据 */ },
  "my_custom_account": { /* 密钥分片数据 */ }
}
```

## 使用示例

### 示例 1: 基本使用

```bash
# 假设你已经通过 BIP-32 派生得到了子密钥
cargo run -- \
  --child-key "a1b2c3d4e5f6789012345678901234567890123456789012345678901234abcd" \
  --account-id "account_0"
```

### 示例 2: 为多个 Ethereum 账户生成密钥分片

```bash
# 账户 0
cargo run -- \
  --child-key "a1b2c3d4e5f6789012345678901234567890123456789012345678901234abcd" \
  --account-id "m/44'/60'/0'/0/0"

# 账户 1
cargo run -- \
  --child-key "b2c3d4e5f6789012345678901234567890123456789012345678901234abcde" \
  --account-id "m/44'/60'/0'/0/1"

# 账户 2
cargo run -- \
  --child-key "c3d4e5f6789012345678901234567890123456789012345678901234abcdef" \
  --account-id "m/44'/60'/0'/0/2"
```

### 示例 3: 5-of-7 配置

```bash
cargo run -- \
  --child-key "a1b2c3d4e5f6789012345678901234567890123456789012345678901234abcd" \
  --account-id "high_security_account" \
  --n-parties 7 \
  --threshold 5
```

### 示例 4: 批量生成多个账户

```bash
#!/bin/bash

# 假设你有一个函数可以派生子密钥
# derive_child_key(path) -> hex_string

PATHS=(
  "m/44'/60'/0'/0/0"
  "m/44'/60'/0'/0/1"
  "m/44'/60'/0'/0/2"
)

for path in "${PATHS[@]}"; do
  # 这里你需要自己实现派生逻辑，得到 child_key
  CHILD_KEY=$(your_derive_function "$path")
  
  echo "Generating key shares for $path..."
  cargo run -- \
    --child-key "$CHILD_KEY" \
    --account-id "$path"
done

echo "All accounts generated!"
```

## 如何获取子密钥

此工具需要输入已派生的子密钥。你可以使用以下方法获取：

### 方法 1: 使用现有的 BIP-32 库

```rust
// 示例代码（使用 bip32 crate）
use bip32::{XPrv, DerivationPath};

let master_key = XPrv::from(...);
let path = "m/44'/60'/0'/0/0".parse::<DerivationPath>()?;
let child_key = master_key.derive_path(&path)?;

let child_key_bytes = child_key.private_key().to_bytes();
println!("Child Key: {}", hex::encode(child_key_bytes));
```

### 方法 2: 使用 HMAC-SHA512（BIP-32 标准）

```rust
use hmac::{Hmac, Mac};
use sha2::Sha512;

// 从主密钥和 chain code 派生子密钥
fn derive_child(parent_key: &[u8; 32], chain_code: &[u8; 32], index: u32) -> [u8; 32] {
    let mut mac = Hmac::<Sha512>::new_from_slice(chain_code).unwrap();
    
    if index >= 0x80000000 {  // hardened
        mac.update(&[0x00]);
        mac.update(parent_key);
    } else {  // normal
        // 需要计算公钥...
    }
    
    mac.update(&index.to_be_bytes());
    let result = mac.finalize().into_bytes();
    
    let mut child_key = [0u8; 32];
    child_key.copy_from_slice(&result[..32]);
    child_key
}
```

### 方法 3: 使用其他工具

你也可以使用其他 HD 钱包工具（如 `ethereumjs-wallet`, `hdkey` 等）先派生出子密钥，然后将其传给此工具。

## 文件格式

密钥分片文件使用 JSON 格式，与现有的 `client_key_shares.json` 和 `service_key_shares.json` 兼容。

每个账户的密钥分片包含：
- `core`: 核心密钥分片数据
  - `i`: 参与方索引
  - `shared_public_key`: 共享公钥
  - `public_shares`: 所有参与方的公钥分片
  - `chain_code`: HD 钱包 chain code
  - `x`: 私钥分片
- `aux`: 辅助数据（用于签名协议）

## 注意事项

⚠️ **安全警告**：
- 子密钥是敏感信息，请妥善保管
- 生成的密钥分片文件应该安全存储并分发给对应的参与方
- 不要将所有密钥分片存储在同一位置
- 建议在安全的环境中运行此工具

💡 **使用建议**：
- 使用有意义的 account_id，如 BIP-32 路径，便于管理
- 定期备份密钥分片文件
- 测试时使用测试网络和测试密钥
- 在生产环境中使用硬件安全模块（HSM）保护主密钥和派生过程

## 与其他工具集成

生成的密钥分片文件可以直接用于：
- `client` 程序：MPC 客户端
- `participant` 程序：MPC 参与方服务
- `sign-service` 程序：MPC 签名服务

配置示例（client.yaml）：

```yaml
local_participant:
  key_share_file: "key_shares_1.json"
  index: 1
  host: "0.0.0.0"
  port: 50051
```

## 故障排除

### 错误: "Child key must be 32 bytes"
确保子密钥是 64 个十六进制字符（32 字节）。

### 错误: "Account already exists, will overwrite"
这是一个警告，表示该 account_id 已存在，新的密钥分片会覆盖旧的。

### 文件权限错误
确保你有权限在当前目录创建和修改文件。

## 开发

### 构建

```bash
cargo build --release
```

### 测试

```bash
cargo test
```

## 相关链接

- [BIP-32: Hierarchical Deterministic Wallets](https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki)
- [BIP-44: Multi-Account Hierarchy](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki)
- [CGGMP21: Threshold ECDSA Protocol](https://eprint.iacr.org/2021/060)

## 许可证

[根据项目主许可证]
