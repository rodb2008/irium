<!-- AI Translation Notice -->
> 注意：本翻译由 AI 生成，尚未经过母语人士审阅。欢迎通过 pull request 提交更正和改进。

# Irium 区块链 (Rust 主网)

[![Rust](https://img.shields.io/badge/Rust-Blockchain-orange?logo=rust)](https://www.rust-lang.org/)
[![算法](https://img.shields.io/badge/算法-SHA256d-blue)](https://github.com/iriumlabs/irium)
[![共识](https://img.shields.io/badge/共识-工作量证明-green)](https://github.com/iriumlabs/irium)
[![许可证](https://img.shields.io/badge/许可证-MIT-lightgrey)](https://github.com/iriumlabs/irium/blob/main/LICENSE)

## Irium (IRM)

Irium 是一个**仅面向生产环境的工作量证明区块链**，专为 IRM 资产设计。

该网络具有以下特点：

- 无测试网
- 无 DNS 依赖（使用签名种子列表引导启动）
- 锁定创世区块，强制执行创始人归属条款
- 固定 **100,000,000 IRM** 总供应量上限

本仓库包含**完整节点、矿工、钱包工具和 SPV 工具的 Rust 实现**。

---

### 共识参数

- 算法：SHA-256d
- 出块时间目标：600 秒
- 难度调整：在高度 16,462 之前每 2016 个区块重新调整，之后使用 LWMA
- 初始区块奖励：50 IRM
- 减半间隔：每 210,000 个区块
- 币基成熟度：100 个区块
- 最大供应量：100,000,000 IRM
- 创世分配：**3,500,000 IRM 使用 CLTV 锁定**

---

### 引导启动

点对点节点发现使用：

- 签名的 `bootstrap/seedlist.txt`
- `anchors.json`
- 缓存在 `bootstrap/seedlist.runtime` 中的运行时节点

---

### 设计目标

- 以主网为优先的架构
- 无 DNS 引导启动
- 对轻客户端友好
- 可选的中继奖励

## 为什么挖掘 Irium？

• 极早期的工作量证明网络
• 独立的 Rust 区块链（非分叉）
• 无 DNS 的点对点发现架构
• 透明的启动分配——无 ICO、无预售、无空投；3,500,000 IRM 创世归属锁定在链上

---

# 快速链接

网站：https://iriumlabs.org

区块浏览器：https://www.iriumlabs.org/explorer

挖矿矿池：pool.iriumlabs.org（3333 端口用于 ASIC，3335 端口用于 CPU/GPU）

Bitcointalk ANN：https://bitcointalk.org/index.php?topic=5572239.0

Telegram：https://t.me/iriumlabs

GitHub 组织：https://github.com/iriumlabs

---

# 挖矿 Irium（最快方式）

### 1. 安装 Rust

访问 https://rustup.rs 安装 Rust，安装完成后打开新终端。

### 2. 下载源代码

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
```

### 3. 编译软件

```bash
source ~/.cargo/env
cargo build --release
```

### 4. 启动节点

```bash
./target/release/iriumd
```

保持此窗口运行。

### 5. 创建钱包地址

打开第二个终端：

```bash
./target/release/irium-wallet init
./target/release/irium-wallet new-address
```

复制生成的地址。

### 6. 开始挖矿

```bash
export IRIUM_MINER_ADDRESS=<YOUR_ADDRESS>
./target/release/irium-miner
```

节点同步完成后即开始挖矿。

---

# 查询余额

```bash
./target/release/irium-wallet balance <YOUR_ADDRESS>
```

---

# 运行节点

```bash
./target/release/iriumd
```

默认数据目录：
- 区块：`~/.irium/blocks`
- 状态：`~/.irium/state`

---

# 故障排除

矿工卡在高度 0 → 节点仍在同步中

矿工无法获取区块模板 → 检查 RPC 连接

无节点连接 → 确保出站 TCP 端口 **38291** 已开放

HTTP 401 → 为节点和矿工设置匹配的 `IRIUM_RPC_TOKEN`

---

# 许可证

MIT 许可证
