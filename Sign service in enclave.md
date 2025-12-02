### Sign Service on AWS Nitro Enclave（方案 A：启动时预加载全部分片）

为满足“云端参与方运行在可信执行环境且一次性加载数以万计 key share”的要求，Sign Service 采用 AWS Nitro Enclave 部署，并选择 **方案 A：实例启动时即预加载全部 key share 分片**。该方案适用于可预估的账户规模，避免会话期间的磁盘/网络抖动，代价是需要在 Enclave 预留充足内存。

**拓扑与边界**
- EC2 Parent（`c7i.metal` 等）承载 Nitro Enclave，Parent 仅负责拉取密文 share、运行 vsock 代理、采集日志，不持有明文密钥。
- Sign Gateway 通过 VPC 私网访问 Parent 上的 gRPC/SSE 代理，代理再经 vsock 转发到 Enclave 内部 `sign-service`。
- VPC 安全组仅放行 Sign Gateway → Parent 的 TLS 流量；Enclave 无直接网络，所有出入口均经 vsock。

**Key Share 预加载流程（Scheme A）**
1. Parent 启动时挂载只读 EBS（加密）或从 S3 下载 `service_key_shares.json.enc` 到 tmpfs。
2. Parent 使用 AWS KMS + Enclave Attestation（vsock attester）完成双向校验后，将密文 share 下发至 Enclave Loader。
3. Enclave Loader 解密（age/KMS session key）、解析全部 account 分片，按 account_id 构建内存字典（可用压缩或 mmap-like chunk 以节省内存）。
4. 预加载完成后立即零化解密缓冲，并把 share 仅保存在 Enclave 内存；Parent 端缓存全部销毁。
5. 在签名会话中，`sign-service` 直接从内存字典获取 share，避免 IO；会话结束后只清理临时 MPC 状态，share 常驻内存直到实例重启。

**通信与代理**
- gRPC：Parent 运行 `enclave-proxy`（可复用 `sign-gateway/src/grpc.rs` 生成的客户端）监听 `0.0.0.0:50051`，与 Sign Gateway 建立 mTLS，随后通过 vsock `CID:16, PORT:50051` 将请求透传。
- SSE：Sign Service 仍需访问 Sign Gateway 的 SSE 端点，Parent 提供 HTTP Client 代理（curl-based relay or Hyper bridge）在 vsock 与外部网络间搬运 SSE 流，并强制只允许白名单 URL。
- 所有请求在进入 Enclave 前再次校验 attestation measurement（PCR hash），防止被调包。

**启动与运维**
1. 使用 Nitro CLI 构建 Enclave 镜像（EIF），包含 `sign-service` 二进制与 `service_key_shares.json` 解析模块。
2. Parent systemd 启动顺序：KMS 解封 → Nitro Enclave run → vsock proxies → health-check（向 Sign Gateway 报告 attestation 和 share 载入完成时间戳）。
3. 失败恢复：若预加载失败（如 JSON 损坏/内存不足），Parent 直接销毁 Enclave 并告警，阻止任何签名请求。
4. 日志：Enclave 内部通过 vsock `log` 通道流向 Parent，再转发到 CloudWatch；敏感数据默认打 redact。

**安全与监控要点**
- 必须记录每次启动的 attestation doc 与 share 的 SHA256 指纹，写入审计链路。
- 设定内存水位报警（例如 70%）和 MPC 会话并发上限，防止 OOM 影响 Enclave。
- Sign Gateway 定期发起健康探测（gRPC ping + SSE 订阅校验），超过阈值自动剔除该 Enclave 节点。
- 应急手册：在 `docs/` 添加“Sign-Service Nitro Enclave Runbook”，涵盖 KMS key rotation、EIF 升级、share 重新加密的步骤。

**方案 A 适用性评估**
- ✅ 优点：无运行期加载抖动，签名时延稳定；share 不需跨边界多次传输。
- ⚠️ 成本：Enclave 内存需匹配 key share 体积（示例：1 万账户约 1.5~2 GB）；重启耗时（minutes）需通过灰度方式滚动。
- 若后续账户规模超过内存预算，可再引入“方案 B：按需流式加载 + LRU”作为扩展。
