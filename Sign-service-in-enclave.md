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

    1. 从 S3 下载 **age-private.key.enc**（KMS 加密的 age 私钥密文）至内存文件系统（`/dev/shm` or `tmpfs`）；

    2. 从 S3 下载 **`service_key_shares.json.age`**（age 公钥加密的 key shares）；

    3. 通过 `socat` VSOCK 通道将两份密文推送到 Enclave (`AGE_KEY_VSOCK_PORT`, `KEY_SHARES_VSOCK_PORT`)，Parent 不持有任何明文。

### 2️⃣ Enclave 启动与 KMS 解封

* **Enclave 启动**：Parent 使用 `nitro-cli run-enclave` 启动 EIF 镜像；S3 密文通过 vsock 传入 `/tmp/age-private.key.enc` 与 `/tmp/service_key_shares.json.age`。

* **Attestation + KMS 解封**：

    * `enclave-entrypoint.sh` 调用 `kmstool_enclave_cli decrypt --ciphertext fileb:///tmp/age-private.key.enc`；

    * 工具自动生成 attestation document 并提交给 KMS 代理（vsock → `nitro-enclaves-vsock-proxy` → KMS）；

    * KMS 验证 PCR measurement 后返回 `PLAINTEXT`（base64），在 Enclave 内部写入 `/dev/shm/age-private.key`；

    * 写入完成后立即 `chmod 600`，并仅在后续 age 解密步骤使用。

### 3️⃣ Enclave 内部解密与加载

* `enclave-entrypoint.sh` 执行：

    1. `age --decrypt -i /dev/shm/age-private.key /tmp/service_key_shares.json.age > /dev/shm/service_key_shares.json`；

    2. 可选地用 `jq empty` 校验 JSON，`sha256sum` 记录指纹；

    3. `shred -u /dev/shm/age-private.key`，确保 age 私钥不再驻留；

    4. 通过环境变量 `SIGN_SERVICE_KEY_SHARE_FILE=/dev/shm/service_key_shares.json` 通知 sign-service；

    5. sign-service 读取此路径构建内存字典。

* 若任一步骤失败（KMS error、age 解密失败、JSON 校验失败），脚本立即退出，Parent 监控探知 Enclave 停止并触发告警。

### 4️⃣ 内存驻留与运行期访问

* 明文 key shares 仅存在于 `/dev/shm/service_key_shares.json` 与 sign-service 内存；

* Age 私钥明文解密完成即销毁；

* Enclave 运行期间不再需要访问 S3/KMS，除非实例重启。

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

1. **下载密文文件**：`run-enclave.sh` 使用 `aws s3 cp` 将 `age-private.key.enc`、`service_key_shares.json.age` 下载到 `tmpfs`；

2. **传输到 Enclave**：脚本通过 `socat FILE -> VSOCK` 将密文推送到 `AGE_KEY_VSOCK_PORT`、`KEY_SHARES_VSOCK_PORT`；

3. **启动 Nitro Enclave**：`nitro-cli run-enclave --eif-path ... --enclave-cid 16`；

4. **Enclave 初始化**：`enclave-entrypoint.sh` 收到密文后执行 KMS 解封 + age 解密；

5. **启动 socat 代理**：同前（gRPC ingress + HTTP egress）；

6. **启动 sign-service**：当环境变量 `SIGN_SERVICE_KEY_SHARE_FILE` 设置完成后执行 sign-service；

7. **健康检查**：Parent 通过 gRPC/socat 轮询，确认 sign-service 就绪。

***

## Secret 注入与脚本

* **Host 脚本**：`scripts/sign-service/run-enclave.sh`：

    * 新增环境变量：`AGE_KEY_S3_URI`, `KEY_SHARES_S3_URI`, `AGE_KEY_VSOCK_PORT`, `KEY_SHARES_VSOCK_PORT`；

    * 自动下载 S3 密文至 `/dev/shm/sign-service-secrets.*`；

    * 使用 `socat` 将文件注入 Enclave；

    * 结束后删除主机上的临时文件。

* **Enclave entrypoint**：`scripts/sign-service/enclave-entrypoint.sh`：

    * 监听 vsock 端口接收密文；

    * 调用 `kmstool_enclave_cli` 解密 age 私钥（输出 base64 PLAINTEXT）；

    * 使用 `age` CLI 解密 `service_key_shares.json.age`；

    * 通过 `SIGN_SERVICE_KEY_SHARE_FILE` 环境变量告诉 sign-service 明文路径；

    * 全流程中 age 私钥仅存于 `/dev/shm`，使用后立刻擦除。

***
