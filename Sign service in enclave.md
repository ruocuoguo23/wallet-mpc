## 概述

Sign Service 采用 AWS Nitro Enclave 部署，实例启动时即预加载全部 key share 分片。

该方案适用于可预估的账户规模。

***

## 拓扑与边界

* EC2 Parent（c7i.metal 等）：承载 Nitro Enclave，负责拉取密文 share、运行 vsock 代理、采集日志，不持有明文密钥。

* Sign Gateway：通过 VPC 私网访问 Parent 上的 gRPC/SSE 代理；~~代理再经 vsock 转发到 Enclave 内部的 sign-service。~~

* VPC 安全组：仅放行 Sign Gateway → Parent 的 TLS 流量；Enclave 无直接网络，所有出入口均经 vsock 通道。

***

## Key Share 预加载流程

### 1️⃣ Parent 拉取密文 share

* Parent 启动时从 S3 下载 `service_key_shares.json.age` 到内存文件系统~~（`/dev/shm` 或 `tmpfs`）~~，S3 对象使用 SSE-KMS 加密。

* 文件为使用 `age` 公钥加密的 JSON，内容包含所有账户的 key share 分片。

### 2️⃣ Enclave 私钥解封（KMS + Enclave Attestation）

* Enclave 启动后生成 attestation document（包含 PCR measurement）。

* KMS 验证 measurement（确保运行的是受信的 Enclave 镜像），返回 age 私钥明文。

### 3️⃣ Enclave Loader 解密与构建内存字典

* Enclave Loader 接收：

    * `age` 私钥（来自 Parent）；

    * `service_key_shares.json.age` 密文（来自 Parent）。

* 使用 `age` 私钥解密密文，得到明文 JSON：

```json
[{"account_id": "A001", "share": "..."},{"account_id": "A002", "share": "..."}]
```

* 解析后构建内存字典（`account_id → share`）。

### 4️⃣ 内存驻留与运行期访问

* 所有 share 常驻 Enclave 内存，Parent 不再持有任何副本。

* Sign-service 在签名会话中直接从内存字典读取 share，避免任何 IO。

* 会话结束后仅清理临时 MPC 状态，share 持续驻留直到实例重启。

***

## 通信与代理

* gRPC：

    * Parent 运行 `enclave-proxy`（可复用 sign-gateway/src/grpc.rs 客户端）监听 `0.0.0.0:50051`；

    * 与 Sign Gateway 建立 mTLS；

    * 请求通过 vsock (`CID:16, PORT:50051`) 透传至 Enclave 内部 sign-service。

* SSE：

    * Sign Service 需访问 Sign Gateway 的 SSE 端点；

    * Parent 提供 HTTP Client 代理（curl-based relay 或 Hyper bridge），在 vsock 与外部网络间搬运 SSE 流；

    * 强制只允许白名单 URL。

* 安全校验：

    * 所有请求在进入 Enclave 前再次校验 attestation measurement（PCR hash），防止被调包。

***

## 启动与运维

1. 使用 Nitro CLI 构建 Enclave 镜像（EIF），包含：

    * sign-service 二进制；

    * Enclave Loader 模块；

    * age 解密逻辑（不包含私钥）。

2. Parent systemd 启动顺序：

    1. 下载密文 share；

    2. 调用 KMS 解封 age 私钥；

    3. 启动 Nitro Enclave；

    4. 通过 vsock 下发私钥与密文；

    5. 启动 vsock proxies；

    6. health-check：向 Sign Gateway 报告 attestation 与 share 载入完成时间戳。

3. 失败恢复：

    * 若预加载失败（如 JSON 损坏、内存不足、KMS 验证失败），Parent 立即销毁 Enclave 并告警，阻止任何签名请求。

4. 日志：

    * Enclave 内部通过 vsock log 通道流向 Parent；

    * Parent 转发到 CloudWatch；

    * 敏感字段默认打 redact。

***

## 安全与监控要点

* 每次启动记录：

    * attestation document；

    * share 文件 SHA256 指纹；

    * age 私钥解封事件（KMS request ID）。

* 设置内存水位报警（如 70%）与 MPC 会话并发上限，防止 OOM。

* Sign Gateway 定期发起健康探测（gRPC ping + SSE 订阅校验），异常节点自动剔除。

* 应急手册（`docs/Sign-Service Nitro Enclave Runbook`）应包含：

    * KMS key rotation；

    * EIF 镜像升级；

    * share 重新加密与重新加载流程。
