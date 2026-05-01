<!-- AI Translation Notice -->
> 注意：本翻译由 AI 生成，尚未经过母语人士审阅。欢迎通过 pull request 提交更正和改进。

# Irium 结算演示：作为自由职业者安全收款

## 场景说明

Alice 是一名自由开发者。Bob 想雇用她以 50 IRM 的价格建设一个网站。

他们此前从未合作过。Alice 不知道交付后 Bob 是否会付款。Bob 不知道付款后 Alice 是否会交付成果。双方都不愿意先行一步。

Irium 结算通过链上托管解决了这个问题。在 Alice 开始任何工作之前，Bob 先将 50 IRM 锁入合约。Alice 知道资金已在链上，Bob 在协议解决之前无法动用。若 Alice 交付成果且证明被接受，她即可获得报酬。若截止日期前未完成交付，Bob 将自动取回资金。

无需银行。无需律师。无需信任。

---

## 第一步 — Alice 和 Bob 线下商定条款

在任何内容上链之前，双方商定：

- 金额：50 IRM
- 交付标准：指定 URL 上可运行的网站
- 截止日期（以区块高度表示，约每 10 分钟一个区块）
- 双方签署的工作描述文件

他们将协议写入文件 `terms.txt`，该文件的哈希值将提交到链上，确保双方无法事后声称条款有所不同。

---

## 第二步 — 生成密钥并创建协议

Bob 生成一次性密钥（秘密原像）。当密钥被揭示时，资金将被解锁：

```bash
# 生成随机 32 字节密钥（Bob 在满意前保密）
SECRET=$(openssl rand -hex 32)

# 计算密钥的哈希值（写入协议，而非密钥本身）
SECRET_HASH=$(printf '%s' "$SECRET" | xxd -r -p | sha256sum | awk '{print $1}')
```

Bob 对条款文件进行哈希处理，将其绑定到链上记录：

```bash
DOCUMENT_HASH=$(sha256sum terms.txt | awk '{print $1}')
```

Bob 创建协议 JSON：

```bash
irium-wallet agreement-create-simple-settlement \
  --agreement-id website-project-001 \
  --creation-time $(date +%s) \
  --party-a "id=alice,name=Alice,role=freelancer" \
  --party-b "id=bob,name=Bob,role=client" \
  --amount 50 \
  --secret-hash $SECRET_HASH \
  --refund-timeout 21500 \
  --document-hash $DOCUMENT_HASH \
  --release-summary "Alice 在区块 21500 之前交付完成的网站" \
  --refund-summary "若 Alice 未在区块 21500 前交付，Bob 取回资金" \
  --out website-project-001.json
```

---

## 第三步 — 与 Alice 共享协议

Bob 将协议 JSON 文件发送给 Alice。Alice 检查内容：

```bash
irium-wallet agreement-inspect website-project-001.json
```

Alice 核实：金额、截止日期、发布摘要是否符合约定。如无异议，双方确认进入下一步。

---

## 第四步 — Bob 为托管账户注资（资金上链）

```bash
irium-wallet agreement-fund website-project-001.json \
  --broadcast \
  --rpc http://localhost:38300
```

50 IRM 现已锁定。Bob 在区块 21500 之前无法取回。Alice 确认资金已在链上等待。

---

## 第五步 — Alice 完成工作

Alice 建设网站。完成后通知 Bob 进行验收。

---

## 第六步 — Bob 验收并揭示密钥

若 Bob 对交付成果满意，他将密钥发送给 Alice：

```bash
# Bob 通过私信发送此值给 Alice
echo "密钥：$SECRET"
```

Alice 使用密钥检查释放资格并领取资金：

```bash
irium-wallet agreement-release-eligibility website-project-001.json \
  --secret $SECRET \
  --destination <ALICE_ADDRESS> \
  --rpc http://localhost:38300
```

50 IRM 转入 Alice 的地址，协议完成结算。

---

## 如果 Bob 不验收怎么办？

**情形 A — Bob 拒绝揭示密钥，尽管交付合格**

Alice 可向 Irium 网络提交交付证明。指定的证明人审核证据后，若证明符合策略，证明人将发布释放密钥。

**情形 B — 超时**

区块 21500 到达，Bob 既未验收也未提出争议，托管资金自动退还给 Bob。

---

## 结果汇总

| 情形 | 结果 |
|------|------|
| Alice 交付，Bob 验收 | Alice 获得 50 IRM |
| Alice 交付，Bob 拒绝揭示密钥 | Alice 可提交证明；证明人可释放资金 |
| Alice 未交付 | Bob 超时后自动取回 50 IRM |
| Bob 超时后无响应 | Bob 自动取回 50 IRM |
| Bob 试图提前取回资金 | 不可能——HTLC 合约阻止此操作 |

---

英文原版：[docs/SETTLEMENT-EXAMPLE.md](../../SETTLEMENT-EXAMPLE.md)
