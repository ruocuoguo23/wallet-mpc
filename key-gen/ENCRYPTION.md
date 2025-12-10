# Age Encryption for Key Shares

## 概述

key-gen 工具现在支持使用 [age](https://github.com/FiloSottile/age) 加密生成的密钥分片文件。这提供了一层额外的安全保护，确保只有持有相应私钥的人才能解密和使用密钥分片。

## 生成 Age 密钥对

在使用加密功能之前，需要为每个参与方生成 age 密钥对：

```bash
# 安装 age CLI 工具
# macOS
brew install age

# 为每个参与方生成密钥对
age-keygen -o party1.key
age-keygen -o party2.key
```

每个密钥文件将包含：
- 私钥（用于解密）：`AGE-SECRET-KEY-...`
- 公钥（用于加密）：`age1...`

## 使用加密功能生成密钥分片

### 基本用法（2方，2-of-2）

```bash
# 提取公钥
PUBKEY1=$(grep "public key:" party1.key | cut -d: -f2 | tr -d ' ')
PUBKEY2=$(grep "public key:" party2.key | cut -d: -f2 | tr -d ' ')

# 生成加密的密钥分片
./key-gen \
  --child-key "620fbd16fdb702ad02c43b9657c1acd0b399d8903e0f321b46ecd81bb69f59c0" \
  --account-id "account_1" \
  --n-parties 2 \
  --threshold 2 \
  --output key_shares \
  --pubkeys "age1pyxskzk50966hxtslha28qunkd6f0aw7am9624w4a7jnt3vvxg0sv532gs,age1cff4n2hgk7sdyjqfnd9nhql555pjf6928fg23gzmrg4exl7tgfpqgz83y5"
```

这将生成：
- `key_shares_1.json.age` - 使用 party1 的公钥加密（Client/Mobile App）
- `key_shares_2.json.age` - 使用 party2 的公钥加密（Sign Service/Enclave）

### 不使用加密（默认行为）

如果不提供 `--pubkeys` 参数，文件将不加密：

```bash
./key-gen \
  --child-key <64_hex_chars> \
  --account-id "account_1" \
  --n-parties 2 \
  --threshold 2 \
  --output key_shares
```

这将生成：
- `key_shares_1.json`
- `key_shares_2.json`
- `key_shares_3.json`

## 解密密钥分片

要解密并使用密钥分片：

```bash
# 解密单个文件
age --decrypt -i party1.key -o key_shares_1.json key_shares_1.json.age

# 或者直接传给程序（假设程序支持从 stdin 读取）
age --decrypt -i party1.key key_shares_1.json.age | your-mpc-program
```

## 安全最佳实践

### 1. 密钥分发策略

- **分离生成和分发**：在安全环境中生成所有密钥分片
- **独立传输通道**：通过不同的安全通道分发每个密钥分片
- **永不共享私钥**：每个参与方只应持有自己的 age 私钥

### 2. 私钥存储

```bash
# 推荐：将私钥存储在安全位置，设置严格权限
chmod 600 party1.key
mv party1.key ~/.age/

# 或使用硬件密钥存储（如 YubiKey）
```

### 3. 轮换策略

定期轮换 age 密钥对：

```bash
# 1. 生成新的密钥对
age-keygen -o party1-new.key

# 2. 解密旧文件
age --decrypt -i party1.key -o key_shares_1.json key_shares_1.json.age

# 3. 使用新公钥重新加密
age --encrypt -r $(grep "public key:" party1-new.key | cut -d: -f2 | tr -d ' ') \
    -o key_shares_1-new.json.age key_shares_1.json

# 4. 安全删除未加密文件
shred -u key_shares_1.json

# 5. 替换旧文件
mv key_shares_1-new.json.age key_shares_1.json.age
```

## 故障排除

### 错误：公钥数量不匹配

```
⚠️  Warning: Number of public keys (2) doesn't match number of parties (3)
   Files will not be encrypted.
```

**解决方案**：确保提供的公钥数量与 `--n-parties` 参数相同。

### 错误：无效的 age 公钥

```
Error: Invalid age public key 'age1xxx...': ...
```

**解决方案**：
1. 验证公钥格式正确（以 `age1` 开头）
2. 确保没有多余的空格或换行符
3. 检查公钥是否从正确的密钥文件中提取

### 无法追加到加密文件

当前版本不支持直接追加到已加密的文件。如需添加新账户到已加密的文件：

```bash
# 1. 解密现有文件
age --decrypt -i party1.key -o key_shares_1.json key_shares_1.json.age

# 2. 移除加密文件（重要！）
rm key_shares_1.json.age

# 3. 运行 key-gen 添加新账户（会检测到未加密文件）
./key-gen --child-key <new_key> --account-id "account_2" --pubkeys "$PUBKEY1,$PUBKEY2,$PUBKEY3"
```

## 示例脚本

完整的工作流脚本：

```bash
#!/bin/bash
set -e

# 配置
N_PARTIES=3
THRESHOLD=2
ACCOUNT_ID="m/44/60/0/0/0"
CHILD_KEY="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

# 1. 生成 age 密钥对（如果不存在）
for i in $(seq 1 $N_PARTIES); do
  if [ ! -f "party${i}.key" ]; then
    age-keygen -o "party${i}.key"
    chmod 600 "party${i}.key"
  fi
done

# 2. 提取公钥
PUBKEYS=""
for i in $(seq 1 $N_PARTIES); do
  PUBKEY=$(grep "public key:" "party${i}.key" | cut -d: -f2 | tr -d ' ')
  if [ -z "$PUBKEYS" ]; then
    PUBKEYS="$PUBKEY"
  else
    PUBKEYS="$PUBKEYS,$PUBKEY"
  fi
done

# 3. 生成加密的密钥分片
./key-gen \
  --child-key "$CHILD_KEY" \
  --account-id "$ACCOUNT_ID" \
  --n-parties $N_PARTIES \
  --threshold $THRESHOLD \
  --output key_shares \
  --pubkeys "$PUBKEYS"

echo "✅ Encrypted key shares generated successfully!"
echo "📁 Files:"
for i in $(seq 1 $N_PARTIES); do
  echo "   - key_shares_${i}.json.age (decrypt with party${i}.key)"
done
```

## 与其他工具集成

### 在签名服务中使用加密的密钥分片

```bash
# 启动签名服务时自动解密
age --decrypt -i ~/.age/party1.key key_shares_1.json.age | \
  sign-service --key-shares /dev/stdin --config config.yaml
```

### Docker 环境中使用

```dockerfile
FROM rust:latest
RUN cargo install age-plugin-yubikey
COPY party1.key /root/.age/
COPY key_shares_1.json.age /app/
CMD age --decrypt -i /root/.age/party1.key /app/key_shares_1.json.age | \
    /app/sign-service
```

## 参考资料

- [age 规范](https://age-encryption.org/)
- [age GitHub 仓库](https://github.com/FiloSottile/age)
- [age Rust crate 文档](https://docs.rs/age/)

