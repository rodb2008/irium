# Irium Blockchain (Rust Mainnet)

<img src="assets/irium-logo.png" alt="Irium Logo" width="160" />

[![Rust](https://img.shields.io/badge/Rust-Blockchain-orange?logo=rust)](https://www.rust-lang.org/)
[![Algorithm](https://img.shields.io/badge/Algorithm-SHA256d-blue)](https://github.com/iriumlabs/irium)
[![Consensus](https://img.shields.io/badge/Consensus-Proof--of--Work-green)](https://github.com/iriumlabs/irium)
[![License](https://img.shields.io/badge/License-MIT-lightgrey)](https://github.com/iriumlabs/irium/blob/main/LICENSE)
[![Mining](https://img.shields.io/badge/Mining-CPU%20%7C%20ASIC-yellowgreen)](https://github.com/iriumlabs/irium)

## Irium (IRM)

Irium is a **production-only Proof-of-Work blockchain** for the IRM asset.

The network launches with:

- No testnet
- No DNS dependency (signed seedlist bootstrap)
- Locked genesis enforcing founder vesting
- Fixed **100,000,000 IRM** supply cap

This repository contains the **Rust implementation of the full node, miner, wallet tools, and SPV utilities.**

---

### Consensus

- Algorithm: SHA-256d
- Block target: 600 seconds
- Difficulty retarget: 2016 blocks until LWMA activation at height 16,462, then LWMA
- Starting subsidy: 50 IRM
- Halving interval: 210,000 blocks
- Coinbase maturity: 100 blocks
- Max supply: 100,000,000 IRM
- Genesis allocation: **3.5M IRM CLTV-locked**

---

### Bootstrap

Peer discovery uses:

- signed `bootstrap/seedlist.txt`
- `anchors.json`
- runtime peers cached in `bootstrap/seedlist.runtime`

---
### Design Goals

- Mainnet-first architecture
- DNS-free bootstrap
- Light-client friendly
- Optional relay rewards

## Why Mine Irium?

• Extremely early Proof-of-Work network  
• Independent Rust blockchain (not a fork)  
• DNS-free peer discovery architecture  
• Transparent launch distribution — no ICO, no presale, no airdrop; 3.5M IRM genesis vesting is locked on-chain  
• Direct mining supported with the official Irium miner

Network issuance beyond the genesis vesting allocation is distributed through Proof-of-Work mining.

---

# Quick Links

Website https://iriumlabs.org

Explorer https://www.iriumlabs.org/explorer

Mining Pool pool.iriumlabs.org (3333 ASIC, 3335 CPU/GPU)

Bitcointalk ANN https://bitcointalk.org/index.php?topic=5572239.0

Telegram https://t.me/iriumlabs

GitHub Organization https://github.com/iriumlabs


---

# Mine Irium (Fastest Way)

The easiest way to mine Irium is using the **official miner included in this repository**.

### 1. Install Rust

Install Rust:

https://rustup.rs

Open a new terminal after installation.

---

### 2. Download the Source

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
```

---

### 3. Build the Software

```bash
source ~/.cargo/env
cargo build --release
```
## Build GPU Miner

```bash
cargo build --release --features gpu --bin irium-miner-gpu
```

---

### 4. Start the Node

Run the full node and let it begin syncing:

```bash
./target/release/iriumd
```

Leave this running.

---

### 5. Create a Wallet Address

Open a second terminal:

```bash
./target/release/irium-wallet init
./target/release/irium-wallet new-address
```

Copy the generated address.

---

### 6. Start Mining

```bash
export IRIUM_MINER_ADDRESS=<YOUR_ADDRESS>

./target/release/irium-miner
```

Mining will begin once the node is synced.

---

# Check Your Balance

```bash
./target/release/irium-wallet balance <YOUR_ADDRESS>
```

---

# Mining With a Pool (Optional)

You can also mine using the public Irium Stratum pool. Use only the public hostname; backend server IPs are not published.

ASIC / modern firmware:

```
stratum+tcp://pool.iriumlabs.org:3333
```

CPU/GPU third-party software miners:

```
stratum+tcp://pool.iriumlabs.org:3335
```

Example Windows CPU miner with cpuminer-opt:

```bash
cpuminer-<your-cpu-build>.exe -a sha256d -o stratum+tcp://pool.iriumlabs.org:3335 -u YOUR_ADDRESS.worker1 -p x -t 4
```

cpuminer-opt Windows binaries are named by CPU architecture, for example `cpuminer-avx2.exe`, `cpuminer-avx2-sha-vaes.exe`, or `cpuminer-sse2.exe`. Open the extracted folder, run `dir *.exe`, then replace `cpuminer-<your-cpu-build>.exe` with the executable you actually have.

GPU miners should use a SHA-256d-capable Stratum client and the same pool URL on port `3335`.

Worker format:

```
IRM_ADDRESS.worker_name
```

Pool mining is optional.
You can always mine directly using the **official Irium miner**.

More details: [`docs/POOL_STRATUM.md`](docs/POOL_STRATUM.md).

---

# Running a Node

Basic node start:

```bash
./target/release/iriumd
```

Default directories:

Blocks:

```
~/.irium/blocks
```

Runtime state:

```
~/.irium/state
```

RPC endpoint:

```
https://127.0.0.1:38300
```

Example status query:

```bash
curl -k https://127.0.0.1:38300/status
```

---

# Resyncing the Node

If the node becomes stuck during sync, delete only the state directory:

```bash
rm -rf ~/.irium/state
```

Do **not delete the blocks directory** unless starting from scratch.

---

# Repository Layout

```
src/          Rust node, miner, wallet and SPV code
bootstrap/    signed seed lists and anchors
configs/      node and consensus configuration
assets/       project branding
systemd/      service templates
scripts/      operational helpers
```

---

# System Services (Optional)

Install services:

```bash
./install.sh
```

Enable miner:

```bash
sudo systemctl enable --now irium-miner.service
```

Environment files:

Node:

```
/etc/irium/iriumd.env
```

Miner:

```
/etc/irium/miner.env
```

---

# Troubleshooting

Miner stuck at height 0
→ node is still syncing

Miner cannot fetch block template
→ check RPC connection

No peers
→ ensure outbound TCP port **38291** is allowed

HTTP 401
→ set matching `IRIUM_RPC_TOKEN` for node and miner

HTTP 429
→ increase RPC rate limit or set authentication token

---

# Development

Run tests:

```bash
cargo test
```

Build release binaries:

```bash
cargo build --release
```

---
## Having Issues?

If you encounter problems starting the node or miner, please check the **Quick Start troubleshooting guide**:

➡ See: [QUICKSTART.md](https://github.com/iriumlabs/irium/blob/main/QUICKSTART.md)

The quickstart guide includes fixes for common issues such as:

• Node not syncing  
• Miner stuck at height 0  
• RPC connection errors  
• HTTP 401 / 429 errors  
• No peers detected  
• Incorrect miner configuration  

Most setup problems can be resolved using the steps in that guide.

---

## Need Help?

If the issue is not covered in `quickstart.md`, you can ask for help here:

Telegram  
[https://t.me/IriumNetwork](https://t.me/iriumlabs)

Bitcointalk  
https://bitcointalk.org/index.php?topic=5572239.0

When asking for help, please include:

• Operating system  
• Miner software used  
• Launch command  
• Relevant log output

# Contributing

Irium is open-source.

Developers interested in contributing to the node, wallet, miner, or ecosystem tools are welcome to submit pull requests.

---

# License

MIT License
