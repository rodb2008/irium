# Seed Node Recruitment — Telegram Post

---

**Irium needs community seed nodes to decentralise network bootstrapping.**

Right now the network bootstraps from 2 seed nodes (both operated by Irium Labs). If both go offline simultaneously, new nodes cannot connect. We want to fix this.

If you run a server with a static IP and are comfortable running a standard iriumd node, you can help.

**Requirements:**
— 1 CPU core, 1 GB RAM, 20 GB disk, 10 Mbps upload
— Static publicly reachable IP
— Open port 38291 (TCP)
— 95%+ uptime commitment

**What you need to run:**
```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
cargo build --release --bin iriumd
export IRIUM_P2P_BIND=0.0.0.0:38291
./target/release/iriumd
```

That is a fully qualified seed node. No mining required, no RPC exposed publicly.

Full guide: https://github.com/iriumlabs/irium/blob/main/docs/SEED-NODE.md

Reply here with your IP and port to apply. We verify each node is reachable before adding it to the signed seedlist.

Target: minimum 6 community seed nodes. Current: 2 (both Irium Labs).

---
*Ibrahim posts this manually. Do not post directly.*
