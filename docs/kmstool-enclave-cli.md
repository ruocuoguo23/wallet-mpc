## 🧩 工具简介

`kmstool-enclave-cli` 是 `kmstool-enclave` 的重写版本。
主要区别在于：

* 旧版 `kmstool-enclave` 需要与父实例上的 `kmstool-instance` 通信。

* 新版 `kmstool-enclave-cli` 不再依赖 `kmstool-instance`，而是直接通过命令行参数接收 AWS 凭证、密文等信息。

这样，任何能调用命令行的编程语言（Python、Go、Rust 等）都可以使用它，无需重写 SDK，极大提高了灵活性。

***

## ⚙️ 安装与集成步骤

### 1️⃣ 构建

```bash
cd bin/kmstool-enclave-cli
./build.sh
```

### 2️⃣ 拷贝生成文件

```bash
cp kmstool_enclave_cli <your_enclave_app_directory>/
cp libnsm.so <your_enclave_app_directory>/
```

### 3️⃣ 修改 Enclave 应用的 Dockerfile

```dockerfile
COPY kmstool_enclave_cli ./
COPY libnsm.so ./
```

或通过设置库路径加载：

```bash
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:<path_to_libnsm.so>
```

***

## 🔐 使用方式

### 1️⃣ 解密（`decrypt`）

参数：

* `--region`：KMS 区域

* `--proxy-port`：vsock-proxy 端口（默认 8000）

* `--aws-access-key-id` / `--aws-secret-access-key` / `--aws-session-token`：AWS 临时凭证

* `--ciphertext`：Base64 编码的密文

* `--key-id`：KMS 密钥 ID（对称密钥可选）

* `--encryption-algorithm`：加密算法（若指定 key-id 必填）

输出：

```plain&#x20;text
PLAINTEXT: <base64-encoded plaintext>
```

Python 示例：

```python
proc = subprocess.Popen(
    [
        "/kmstool_enclave_cli", "decrypt",
        "--region", "us-east-1",
        "--proxy-port", "8000",
        "--aws-access-key-id", access_key_id,
        "--aws-secret-access-key", secret_access_key,
        "--aws-session-token", token,
        "--ciphertext", ciphertext,
    ],
    stdout=subprocess.PIPE
)
result = proc.communicate()[0].decode()
plaintext_b64 = result.split(":")[1].strip()
```

***

### 2️⃣ 生成数据密钥（`genkey`）

参数：

* `--key-id`：KMS 密钥 ID

* `--key-spec`：密钥规格（`AES-256` 或 `AES-128`）

* 其他参数同上

输出：

```plain&#x20;text
CIPHERTEXT: <base64-encoded encrypted datakey>
PLAINTEXT: <base64-encoded plaintext datakey>
```

***

### 3️⃣ 生成随机数（`genrandom`）

参数：

* `--length`：随机字节长度

* 其他参数同上

输出：

```plain&#x20;text
PLAINTEXT: <base64-encoded random bytes>
```

***

## 🧰 常见问题

### ❌ 缺少 CA 证书

如果运行时报错：

```plain&#x20;text
Error initializing trust store ...
Failed to set ca_path: (null)
```

说明镜像中缺少根证书。

解决方法：

* 使用带证书的基础镜像，如 `amazonlinux:2`。

* 或在 Dockerfile 中安装：

```dockerfile
RUN apt-get update && apt-get install -y ca-certificates
```

***

## ✅ 总结

核心优势：

* 不依赖 `kmstool-instance`

* 语言无关，可通过任何 shell 调用

* 适合在 Nitro Enclave 内部安全访问 AWS KMS

