# KMS Attestation Policy 配置指南

本文档详细说明如何配置 AWS KMS Key Policy，使其仅允许通过 Nitro Enclave attestation 验证的 Enclave 解密密钥。

## 概述

AWS Nitro Enclave 提供基于硬件的加密证明（Cryptographic Attestation）。当 Enclave 调用 KMS 解密操作时，`kmstool_enclave_cli` 会自动生成包含 PCR (Platform Configuration Registers) 值的 attestation document。KMS 会验证该 document 并检查 PCR 值是否符合 Key Policy 中的条件。

### PCR 寄存器说明

| PCR | 内容 | 用途 |
|-----|------|------|
| **PCR0** | Enclave 镜像的 SHA384 哈希 | 验证 EIF 镜像完整性（最常用） |
| **PCR1** | Linux 内核和引导 ramdisk 的哈希 | 验证引导组件 |
| **PCR2** | 用户空间应用程序的哈希 | 验证应用层 |
| **PCR3** | IAM Role ARN 的哈希 | 验证实例角色（可选增强） |
| **PCR4** | 实例 ID 的哈希 | 绑定到特定实例（较少使用） |
| **PCR8** | 签名证书的哈希 | 验证 EIF 签名（如果使用） |

---

## 第一步：获取 Enclave PCR0 值

构建 EIF 镜像时会输出 PCR 值：

```bash
# 构建 Enclave 镜像
./scripts/sign-service/build-eif.sh

# 输出示例：
# Enclave Image successfully created.
# {
#   "Measurements": {
#     "HashAlgorithm": "Sha384 { ... }",
#     "PCR0": "287b24930a9f0fe14b01a71ecdc00d8be8fad90f9834d547158854b8279c74095c43f8d7f047714e98deb7903f20e3dd",
#     "PCR1": "aca6e62ffbf5f7deccac452d7f8cee1b94048faf62afc16c8ab68c9fed8c38010c73a669f9a36e596032f0b973d21895",
#     "PCR2": "0315f483ae1220b5e023d8c80ff1e135edcca277e70860c31f3003b36e3b2aaec5d043c9ce3a679e3bbd5b3b93b61d6f"
#   }
# }
```

**重要**：请保存 `PCR0` 值（96 字符的十六进制字符串），后续配置 KMS Key Policy 需要用到。

---

## 第二步：配置 KMS Key Policy

### 完整 Key Policy 示例

创建或更新 KMS Key 时，使用以下策略：

```json
{
  "Version": "2012-10-17",
  "Id": "sign-service-enclave-key-policy",
  "Statement": [
    {
      "Sid": "AllowEnclaveDecrypt",
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::ACCOUNT_ID:role/NitroEnclaveInstanceRole"
      },
      "Action": "kms:Decrypt",
      "Resource": "*",
      "Condition": {
        "StringEqualsIgnoreCase": {
          "kms:RecipientAttestation:ImageSha384": "YOUR_PCR0_VALUE_HERE"
        }
      }
    },
    {
      "Sid": "AllowEncrypt",
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::ACCOUNT_ID:role/KeyManagementRole"
      },
      "Action": "kms:Encrypt",
      "Resource": "*"
    },
    {
      "Sid": "AllowKeyAdministration",
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::ACCOUNT_ID:role/KMSAdminRole"
      },
      "Action": [
        "kms:Create*",
        "kms:Describe*",
        "kms:Enable*",
        "kms:List*",
        "kms:Put*",
        "kms:Update*",
        "kms:Revoke*",
        "kms:Disable*",
        "kms:Get*",
        "kms:Delete*",
        "kms:TagResource",
        "kms:UntagResource",
        "kms:ScheduleKeyDeletion",
        "kms:CancelKeyDeletion"
      ],
      "Resource": "*"
    },
    {
      "Sid": "AllowKeyUsageViaGrant",
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::ACCOUNT_ID:role/KMSAdminRole"
      },
      "Action": [
        "kms:CreateGrant",
        "kms:ListGrants",
        "kms:RevokeGrant"
      ],
      "Resource": "*",
      "Condition": {
        "Bool": {
          "kms:GrantIsForAWSResource": "true"
        }
      }
    }
  ]
}
```

### 参数替换说明

| 占位符 | 替换为 |
|--------|--------|
| `ACCOUNT_ID` | 你的 AWS 账户 ID（12 位数字） |
| `NitroEnclaveInstanceRole` | EC2 实例的 IAM Role 名称 |
| `KeyManagementRole` | 用于加密数据的管理角色 |
| `KMSAdminRole` | KMS 密钥管理员角色 |
| `YOUR_PCR0_VALUE_HERE` | 构建 EIF 时获取的 PCR0 值 |

---

## 第三步：使用 AWS CLI 创建 KMS Key

### 3.1 保存策略文件

将上述策略保存为 `kms-key-policy.json`，并替换占位符。

### 3.2 创建 KMS Key

```bash
# 创建 KMS 密钥
aws kms create-key \
    --description "Sign Service Enclave Age Key" \
    --key-usage ENCRYPT_DECRYPT \
    --key-spec SYMMETRIC_DEFAULT \
    --policy file://kms-key-policy.json \
    --tags TagKey=Application,TagValue=sign-service \
    --region us-east-1

# 输出示例：
# {
#     "KeyMetadata": {
#         "KeyId": "1234abcd-12ab-34cd-56ef-1234567890ab",
#         "Arn": "arn:aws:kms:us-east-1:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab",
#         ...
#     }
# }
```

### 3.3 创建别名（可选但推荐）

```bash
aws kms create-alias \
    --alias-name alias/sign-service-enclave-key \
    --target-key-id 1234abcd-12ab-34cd-56ef-1234567890ab \
    --region us-east-1
```

---

## 第四步：加密 Age 私钥

使用 KMS 加密 age 私钥：

```bash
# 方法 1: 使用 Key ID
aws kms encrypt \
    --key-id 1234abcd-12ab-34cd-56ef-1234567890ab \
    --plaintext fileb://age-private.key \
    --output text \
    --query CiphertextBlob \
    --region us-east-1 | base64 -d > age-private.key.enc

# 方法 2: 使用别名
aws kms encrypt \
    --key-id alias/sign-service-enclave-key \
    --plaintext fileb://age-private.key \
    --output text \
    --query CiphertextBlob \
    --region us-east-1 | base64 -d > age-private.key.enc

# 上传到 S3
aws s3 cp age-private.key.enc s3://your-bucket/enclave/age-private.key.enc

# 删除本地明文私钥
shred -u age-private.key
```

---

## 第五步：配置 EC2 Instance Role

EC2 实例角色需要以下权限：

### IAM Policy

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "AllowKMSDecrypt",
      "Effect": "Allow",
      "Action": "kms:Decrypt",
      "Resource": "arn:aws:kms:us-east-1:ACCOUNT_ID:key/YOUR_KEY_ID"
    },
    {
      "Sid": "AllowS3Read",
      "Effect": "Allow",
      "Action": [
        "s3:GetObject"
      ],
      "Resource": [
        "arn:aws:s3:::your-bucket/enclave/*"
      ]
    }
  ]
}
```

**注意**：即使 IAM Policy 允许 `kms:Decrypt`，如果 attestation document 中的 PCR0 与 KMS Key Policy 中配置的值不匹配，解密仍会被拒绝。

---

## 调试模式 (Debug Mode)

### 测试阶段使用全零 PCR0

在 debug 模式下运行 Enclave 时，PCR0 固定为全零。可以临时配置 Key Policy 允许调试：

```json
{
  "Sid": "AllowDebugEnclaveDecrypt",
  "Effect": "Allow",
  "Principal": {
    "AWS": "arn:aws:iam::ACCOUNT_ID:role/NitroEnclaveInstanceRole"
  },
  "Action": "kms:Decrypt",
  "Resource": "*",
  "Condition": {
    "StringEqualsIgnoreCase": {
      "kms:RecipientAttestation:ImageSha384": "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
    }
  }
}
```

### 启动 Debug Enclave

```bash
nitro-cli run-enclave \
    --eif-path sign-service.eif \
    --enclave-cid 16 \
    --memory 512 \
    --cpu-count 2 \
    --debug-mode
```

⚠️ **警告**：生产环境必须移除 debug 条件，使用实际的 PCR0 值！

---

## 增强安全配置（可选）

### 同时验证 PCR0 和 PCR3

添加 PCR3 可以确保只有特定 IAM Role 的实例才能解密：

```json
{
  "Sid": "AllowEnclaveDecryptWithRoleCheck",
  "Effect": "Allow",
  "Principal": {
    "AWS": "arn:aws:iam::ACCOUNT_ID:role/NitroEnclaveInstanceRole"
  },
  "Action": "kms:Decrypt",
  "Resource": "*",
  "Condition": {
    "StringEqualsIgnoreCase": {
      "kms:RecipientAttestation:ImageSha384": "YOUR_PCR0_VALUE",
      "kms:RecipientAttestation:PCR3": "YOUR_PCR3_VALUE"
    }
  }
}
```

### 获取 PCR3 值

PCR3 是 IAM Role ARN 的 SHA384 哈希。可以通过以下方式计算：

```bash
# 计算 IAM Role ARN 的 SHA384 哈希
echo -n "arn:aws:iam::123456789012:role/NitroEnclaveInstanceRole" | sha384sum
# 输出: abcdef1234567890... (96 字符)
```

或者在 Enclave 中运行时从 attestation document 获取实际值。

---

## 支持多个 Enclave 版本

如果需要支持多个 EIF 版本（如蓝绿部署），可以使用多个条件：

```json
{
  "Sid": "AllowMultipleEnclaveVersions",
  "Effect": "Allow",
  "Principal": {
    "AWS": "arn:aws:iam::ACCOUNT_ID:role/NitroEnclaveInstanceRole"
  },
  "Action": "kms:Decrypt",
  "Resource": "*",
  "Condition": {
    "StringEqualsIgnoreCase": {
      "kms:RecipientAttestation:ImageSha384": [
        "PCR0_VALUE_VERSION_1",
        "PCR0_VALUE_VERSION_2"
      ]
    }
  }
}
```

---

## 更新 KMS Key Policy

当重新构建 EIF 后，PCR0 会改变，需要更新 Key Policy：

```bash
# 1. 获取当前策略
aws kms get-key-policy \
    --key-id YOUR_KEY_ID \
    --policy-name default \
    --output text > current-policy.json

# 2. 编辑策略，更新 PCR0 值
vim current-policy.json

# 3. 应用新策略
aws kms put-key-policy \
    --key-id YOUR_KEY_ID \
    --policy-name default \
    --policy file://current-policy.json \
    --region us-east-1
```

---

## 故障排除

### 错误：AccessDeniedException

**原因**：PCR0 值不匹配或 attestation document 无效。

**检查步骤**：

1. 确认 EIF 未重新构建（重新构建会改变 PCR0）
2. 确认 Key Policy 中的 PCR0 值正确
3. 确认不是 debug 模式（debug 模式 PCR0 = 全零）

### 错误：KMS proxy connection failed

**原因**：vsock-proxy 未运行或端口不正确。

**检查步骤**：

```bash
# 在 Host 上检查 vsock-proxy 是否运行
ps aux | grep vsock-proxy

# 确认代理配置
vsock-proxy 5000 kms.us-east-1.amazonaws.com 443
```

### 错误：InvalidCiphertextException

**原因**：密文损坏或使用了错误的 KMS Key。

**检查步骤**：

1. 确认密文是用同一个 KMS Key 加密的
2. 确认密文传输过程中未损坏

---

## 相关资源

- [AWS KMS Cryptographic Attestation](https://docs.aws.amazon.com/enclaves/latest/user/kms.html)
- [AWS KMS Key Policies](https://docs.aws.amazon.com/kms/latest/developerguide/key-policies.html)
- [Nitro Enclaves Attestation](https://docs.aws.amazon.com/enclaves/latest/user/set-up-attestation.html)
- [kmstool-enclave-cli 使用指南](./kmstool-enclave-cli.md)

