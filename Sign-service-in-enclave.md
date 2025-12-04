## 概述

Sign Service 采用 AWS Nitro Enclave 部署，实例启动时即预加载全部 key share 分片。

新增的 `socat` 双向桥在不改动 sign-service 代码的前提下解决网络隔离：

* 将 Enclave 内部的 vsock 端口映射到 Parent 对外暴露的 gRPC 端口 `50051`；
* 同步提供一个 vsock → Parent → VPC 的出站隧道，使 Sign Service 可以访问外部 `8080`（Sign Gateway SSE）端点。

该方案适用于可预估的账户规模。

***

## 拓扑与边界

* EC2 Parent（c7i.metal 等）：承载 Nitro Enclave，负责拉取密文 share、运行 `socat` vsock↔TCP 桥、采集日志，不持有明文密钥。

* Sign Gateway：通过 VPC 私网直接访问 Parent 上的 gRPC 50051（由 `socat` 转发到 Enclave 内部的 sign-service）。

* VPC 安全组：仅放行 Sign Gateway → Parent 的 TLS 流量；Enclave 无直接网络，所有出入口均经 vsock + `socat` 通道。

***

## Age 私钥的安全存储与访问

### 存储架构

* **KMS CMK 加密**：使用专用 AWS KMS Customer Master Key (CMK) 加密 age 私钥。

* **密文存储位置**：加密后的 age 私钥密文存储在 S3 或 AWS Systems Manager Parameter Store（建议 SecureString 类型）。

* **访问控制策略**：KMS Key Policy 配置 `kms:Decrypt` 条件，仅允许特定 PCR0 measurement 的 Enclave 解密：

```json
{
  "Sid": "AllowEnclaveDecrypt",
  "Effect": "Allow",
  "Principal": {
    "AWS": "arn:aws:iam::ACCOUNT_ID:role/NitroEnclaveRole"
  },
  "Action": "kms:Decrypt",
  "Resource": "*",
  "Condition": {
    "StringEqualsIgnoreCase": {
      "kms:RecipientAttestation:ImageSha384": "ENCLAVE_PCR0_HASH"
    }
  }
}
```

* **安全边界**：Parent EC2 实例可以下载密文，但无法解密；只有通过 attestation 验证的 Enclave 内部进程才能获取明文 age 私钥。

***

## Key Share 预加载流程

### 1️⃣ Parent 准备阶段

* Parent 启动时执行：

    1. 从 S3/Parameter Store 下载 **age 私钥密文**（KMS 加密）；

    2. 从 S3 下载 **`service_key_shares.json.age`**（使用 age 公钥加密的 key shares）；

    3. 两个密文文件均加载到内存文件系统（`tmpfs`），Parent 不持有任何明文。

### 2️⃣ Enclave 启动与 KMS 解封

* **Enclave 启动**：Parent 使用 `nitro-cli run-enclave` 启动 EIF 镜像。

* **Attestation 生成**：Enclave 内部的 `kmstool-enclave-cli` 自动生成 attestation document，包含：

    * PCR0：EIF 镜像 SHA384 measurement；

    * PCR1/PCR2：内核与应用哈希（可选）；

    * 公钥和签名，证明来自真实 Nitro Enclave。

* **KMS 解密请求**：

    * Enclave 通过 vsock 代理（Parent 提供的 KMS 代理服务）调用 `kmstool-enclave-cli decrypt`；

    * 请求携带 attestation document 和 age 私钥密文；

    * KMS 验证 attestation 中的 PCR0 是否匹配 Key Policy 白名单；

    * 验证通过后，KMS 返回明文 age 私钥。

* **密钥传递**：`kmstool-enclave-cli` 将明文 age 私钥写入 Enclave 内存（通过 stdout 或临时 ramdisk `/tmp/age.key`）。

### 3️⃣ Enclave 内部解密与加载

* **初始化脚本**（`enclave-entrypoint.sh`）在启动 sign-service 前执行：

    1. 调用 `kmstool-enclave-cli` 解密 age 私钥密文 → 获得明文 age 私钥；

    2. 从 vsock 接收 Parent 传入的 `service_key_shares.json.age` 密文；

    3. 使用 `age` CLI 或 Rust `age` crate 解密 key shares：

       ```bash
       echo "$AGE_PRIVATE_KEY" | age --decrypt -i - service_key_shares.json.age > /tmp/service_key_shares.json
       ```

    4. 验证解密后的 JSON 格式正确；

    5. 将明文 key shares 写入内存文件系统 `/dev/shm/service_key_shares.json`；

    6. **立即擦除 age 私钥**：覆写内存中的 age 私钥副本（`shred` 或 `memset`）。

* **Sign-service 启动**：配置文件指向 `/dev/shm/service_key_shares.json`，读取明文 key shares 并构建内存字典。

### 4️⃣ 内存驻留与运行期访问

* 所有 key share 常驻 Enclave 内存，Parent 不再持有任何明文副本。

* Sign-service 在签名会话中直接从内存字典读取 share，避免任何 IO。

* Age 私钥在完成 key shares 解密后立即销毁，不驻留运行时内存。

* 会话结束后仅清理临时 MPC 状态，share 持续驻留直到实例重启。

***

## 通信与代理

* gRPC：

    * Parent 启动 `socat TCP4-LISTEN:50051,fork VSOCK-CONNECT:16:50051`，对外暴露 gRPC 端口，但实际流量仍进入 Enclave 内部。

    * Sign Gateway 与 Parent 建立 mTLS，所有请求透传至 Sign Service；Sign Service 无需感知 `socat` 的存在。

* SSE / HTTP Egress：

    * Sign Service 需访问 Sign Gateway 的 SSE 端点（外部 `8080`）。

    * Parent 同步运行 `socat VSOCK-LISTEN:8080,fork TCP4:sign-gateway:8080`（或指定 IP），为 Enclave 提供出站 HTTP Client 通道。

    * 白名单 URL 由 Parent 侧代理限制，确保 Enclave 仅能访问 Sign Gateway。

* 安全校验：

    * `socat` 启动前后均校验 Enclave attestation measurement，防止 Parent 代理被调包。

***

## 启动与运维

### 构建 Enclave 镜像 (EIF)

使用 Nitro CLI 构建 Enclave 镜像，包含：

* **sign-service 二进制**：核心签名服务进程；

* **kmstool-enclave-cli**：AWS Nitro Enclaves SDK 提供的 KMS 客户端，用于 attestation-based 解密；

* **age CLI 或 Rust age crate**：用于解密 `service_key_shares.json.age`；

* **enclave-entrypoint.sh**：启动脚本，协调 KMS 解密 → age 解密 → sign-service 启动的流程。

构建完成后提取 **PCR0 measurement**（EIF SHA384 哈希），用于配置 KMS Key Policy。

### Parent 启动顺序

1. **下载密文文件**：

    * 从 S3/Parameter Store 下载 age 私钥密文（KMS 加密）；

    * 从 S3 下载 `service_key_shares.json.age`（age 加密）。

2. **启动 KMS 代理**（可选，取决于 `kmstool-enclave-cli` 实现）：

    * 如需通过 vsock 代理 KMS API，Parent 启动 `vsock-proxy` 服务，监听 vsock 端口（如 `5000`），转发到 `kms.{region}.amazonaws.com:443`。

3. **启动 Nitro Enclave**：

    * `nitro-cli run-enclave --eif-path sign-service.eif --memory 4096 --cpu-count 2 --enclave-cid 16`；

    * Enclave 启动后，执行 `enclave-entrypoint.sh`。

4. **Enclave 内部初始化**（由 `enclave-entrypoint.sh` 自动执行）：

    1. **KMS 解密 age 私钥**：

       ```bash
       # 通过 vsock 从 Parent 接收 age 私钥密文（或从预挂载路径读取）
       AGE_KEY_PLAINTEXT=$(kmstool-enclave-cli decrypt \
         --region us-east-1 \
         --proxy-port 5000 \
         --ciphertext file:///mnt/age-key.enc)
       ```

    2. **Age 解密 key shares**：

       ```bash
       echo "$AGE_KEY_PLAINTEXT" | age --decrypt -i - \
         /mnt/service_key_shares.json.age > /dev/shm/service_key_shares.json
       ```

    3. **擦除 age 私钥**：

       ```bash
       unset AGE_KEY_PLAINTEXT
       # 如使用临时文件，则 shred -u /tmp/age.key
       ```

    4. **验证 key shares 格式**：

       ```bash
       if ! jq empty /dev/shm/service_key_shares.json 2>/dev/null; then
         log "ERROR: Decrypted key shares invalid"
         exit 1
       fi
       ```

5. **启动 socat 代理**：

    * Parent 启动 `socat TCP4-LISTEN:50051,fork VSOCK-CONNECT:16:50051`（gRPC ingress）；

    * Parent 启动 `socat VSOCK-LISTEN:8080,fork TCP4:sign-gateway:8080`（HTTP egress）。

6. **启动 sign-service**：

    * Enclave 内 `enclave-entrypoint.sh` 执行 `sign-service config/sign-service.yaml`；

    * 配置文件中 `key_share_file` 指向 `/dev/shm/service_key_shares.json`。

7. **健康检查**：

    * Parent 通过 vsock 或 gRPC 探测 sign-service 就绪状态；

    * 向 Sign Gateway 报告 attestation document、PCR0 measurement、key shares 加载时间戳。

### 失败恢复与监控

* **KMS 解密失败**：

    * 若 attestation 验证失败（PCR0 不匹配）或 KMS 不可达，`kmstool-enclave-cli` 返回非零退出码；

    * `enclave-entrypoint.sh` 检测到错误后立即退出，Parent 监控 Enclave 状态，终止并告警；

    * CloudWatch 记录 KMS 请求 ID、错误码、attestation 内容。

* **Age 解密失败**：

    * 若 `service_key_shares.json.age` 损坏或 age 密钥不匹配，解密输出空或格式错误；

    * `enclave-entrypoint.sh` 验证 JSON 有效性，失败则退出；

    * Parent 销毁 Enclave，阻止任何签名请求，触发告警。

* **Socat 进程异常**：

    * Parent 监控 `socat` 进程状态，退出时自动重启并重新运行健康检查。

* **日志与审计**：

    * Enclave 内部日志通过 vsock log 通道流向 Parent；

    * Parent 转发到 CloudWatch Logs，包含：

        * KMS 请求 ID 和 attestation document；

        * Key shares 文件 SHA256 指纹；

        * 解密成功/失败时间戳；

        * 敏感字段（age 私钥明文）默认 redact。

***

## 安全与监控要点

### 启动审计

* 每次启动记录：

    * **Attestation document**：完整的 PCR0/PCR1/PCR2 measurement 和签名；

    * **KMS 请求 ID**：`kmstool-enclave-cli` 返回的 KMS Decrypt API request ID；

    * **Age 私钥解封事件**：成功/失败状态、时间戳、KMS 响应延迟；

    * **Key shares 文件 SHA256**：解密前后的密文和明文指纹，用于完整性验证；

    * **Enclave 启动参数**：内存大小、CPU 核数、CID、挂载路径。

### 运行时监控

* **内存水位告警**：设置 Enclave 内存使用率阈值（如 70%），防止 OOM 导致 key shares 丢失。

* **MPC 会话并发上限**：限制同时进行的签名会话数量，避免资源耗尽。

* **健康探测**：

    * Sign Gateway 定期发起 gRPC `Ping` 请求；

    * 定期订阅 SSE 端点，验证出站连接正常；

    * 异常节点自动从负载均衡池剔除。

* **KMS 可用性监控**：

    * 统计 KMS API 调用成功率和延迟（P50/P99）；

    * 设置告警：连续失败 > 3 次或延迟 > 5s。

### 运维操作手册

#### 1. 初始化：生成与加密 age 私钥

```bash
# 1. 生成 age 密钥对
age-keygen -o age-private.key
AGE_PUBKEY=$(grep "public key:" age-private.key | cut -d: -f2 | tr -d ' ')

# 2. 使用 KMS 加密私钥
aws kms encrypt \
  --key-id alias/sign-service-age-key \
  --plaintext fileb://age-private.key \
  --output text \
  --query CiphertextBlob | base64 -d > age-private.key.enc

# 3. 上传密文到 S3
aws s3 cp age-private.key.enc s3://your-bucket/enclave/age-private.key.enc

# 4. 安全删除明文私钥
shred -u age-private.key

# 5. 使用公钥加密 key shares
./key-gen --child-key <key> --account-id <id> --pubkeys "$AGE_PUBKEY" ...
```

#### 2. EIF 镜像升级与 PCR 更新

```bash
# 1. 构建新的 EIF
nitro-cli build-enclave --docker-uri sign-service:latest --output-file sign-service.eif

# 2. 提取 PCR0 measurement
PCR0=$(nitro-cli describe-eif --eif-path sign-service.eif | jq -r '.Measurements.PCR0')

# 3. 更新 KMS Key Policy
aws kms put-key-policy \
  --key-id alias/sign-service-age-key \
  --policy-name default \
  --policy file://kms-policy.json  # 包含新的 PCR0

# 4. 部署新 EIF
# 使用滚动更新或蓝绿部署策略，确保至少 1 个旧版本实例在线
```

#### 3. Age 密钥轮换

```bash
# 1. 生成新的 age 密钥对
age-keygen -o age-private-v2.key
AGE_PUBKEY_V2=$(grep "public key:" age-private-v2.key | cut -d: -f2 | tr -d ' ')

# 2. 解密现有 key shares（需在 Enclave 内或临时安全环境）
age --decrypt -i age-private.key service_key_shares.json.age > service_key_shares.json

# 3. 使用新公钥重新加密
age --encrypt -r "$AGE_PUBKEY_V2" service_key_shares.json > service_key_shares-v2.json.age

# 4. 加密新私钥并上传
aws kms encrypt --key-id alias/sign-service-age-key \
  --plaintext fileb://age-private-v2.key --query CiphertextBlob \
  --output text | base64 -d > age-private-v2.key.enc
aws s3 cp age-private-v2.key.enc s3://your-bucket/enclave/age-private.key.enc
aws s3 cp service_key_shares-v2.json.age s3://your-bucket/enclave/service_key_shares.json.age

# 5. 滚动重启所有 Enclave 实例
# 新实例将自动使用新密钥
```

#### 4. KMS CMK 轮换

```bash
# 1. 启用 KMS 自动轮换（推荐）
aws kms enable-key-rotation --key-id alias/sign-service-age-key

# 注意：KMS 自动轮换不改变 Key ID，密文无需重新加密
# 如需手动轮换（生成新 CMK），需重新加密所有 age 私钥密文
```

#### 5. 应急响应：密钥泄露

```bash
# 1. 立即禁用 KMS CMK
aws kms disable-key --key-id alias/sign-service-age-key

# 2. 终止所有 Enclave 实例
nitro-cli terminate-enclave --all

# 3. 生成新的 age 密钥对和 KMS CMK
aws kms create-key --description "Sign Service Age Key v2"

# 4. 重新生成所有 key shares（需使用 key-gen 工具）
# 5. 部署新 EIF（包含新的 PCR measurement）
# 6. 更新所有配置，重新启动服务
```

### 合规与审计

* **CloudTrail 日志**：记录所有 KMS API 调用（`kms:Decrypt`），包含 attestation document 和 PCR 值。

* **Attestation 归档**：每次启动的 attestation document 归档到 S3，保留 90 天用于审计。

* **定期安全评估**：

    * 每季度审查 KMS Key Policy，确认 PCR 白名单准确；

    * 验证 age 密钥轮换流程可用性；

    * 测试应急响应手册的完整性。
