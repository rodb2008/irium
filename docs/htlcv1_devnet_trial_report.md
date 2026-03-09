# HTLCv1 Devnet Trial Report (Controlled, Activation-Gated)

Date: 2026-03-09

## Topology
- Node A host: `irium-vps` (`207.244.247.86`)
- Node B host: `irium-eu` (`157.173.116.134`)
- Isolated trial workdir on both hosts: `/tmp/htlc-live-trial`
- Isolated trial data dirs:
  - Node A: `/home/irium/.htlc-devtrial/node1`
  - Node B: `/home/irium/.htlc-devtrial/node2`

## Activation Height
- `IRIUM_HTLCV1_ACTIVATION_HEIGHT=5`
- Mainnet services were not modified.

## Trial Genesis/Anchor Isolation
- Trial used isolated `configs/genesis-locked.json` in `/tmp/htlc-live-trial` only.
- Trial anchor set was generated and signed with a temporary trial signer key in:
  - `/tmp/htlc-live-trial-keys/anchor_signer`
- Production repo/data were not reused for trial consensus state.

## Commands Executed (Key)

### Build
```bash
ssh irium-vps 'cd /tmp/htlc-live-trial && source $HOME/.cargo/env 2>/dev/null || true && cargo build --release --bin iriumd --bin irium-miner'
# irium-eu binary staged from vps build artifact for parity
```

### Node launch
```bash
# Node A (vps)
nohup env   IRIUM_NODE_CONFIG=/home/irium/.htlc-devtrial/node1.json   IRIUM_NODE_HOST=127.0.0.1 IRIUM_NODE_PORT=58400   IRIUM_STATUS_HOST=127.0.0.1 IRIUM_STATUS_PORT=58480   IRIUM_RPC_TOKEN=trialtoken   IRIUM_HTLCV1_ACTIVATION_HEIGHT=5   IRIUM_SEEDLIST_ALLOW_UNSIGNED=1   IRIUM_ALLOW_LOCAL_SEED_FALLBACK=1   IRIUM_NODE_WALLET_FILE=/home/irium/.htlc-devtrial/node1/wallet.core.json   ./target/release/iriumd > /home/irium/.htlc-devtrial/logs/node1.log 2>&1 &

# Node B (eu)
nohup env   IRIUM_NODE_CONFIG=/home/irium/.htlc-devtrial/node2.json   IRIUM_NODE_HOST=127.0.0.1 IRIUM_NODE_PORT=58401   IRIUM_STATUS_HOST=127.0.0.1 IRIUM_STATUS_PORT=58481   IRIUM_RPC_TOKEN=trialtoken   IRIUM_HTLCV1_ACTIVATION_HEIGHT=5   IRIUM_SEEDLIST_ALLOW_UNSIGNED=1   IRIUM_ALLOW_LOCAL_SEED_FALLBACK=1   IRIUM_NODE_WALLET_FILE=/home/irium/.htlc-devtrial/node2/wallet.core.json   ./target/release/iriumd > /home/irium/.htlc-devtrial/logs/node2.log 2>&1 &
```

## Runtime Evidence

### Node A status
```json
{"height":0,"genesis_hash":"3eebf1c383bff87f0be4caf70acfe57e4f076f8050f24f77e62522bc2401e1c1","peer_count":0,"anchor_loaded":true}
```

### Node B status
```json
{"height":0,"genesis_hash":"3eebf1c383bff87f0be4caf70acfe57e4f076f8050f24f77e62522bc2401e1c1","peer_count":0,"anchor_loaded":true}
```

### P2P dial failures observed
- Node A: `outbound 157.173.116.134:59292 failed: ... connect timeout after 8s`
- Node B: `outbound 207.244.247.86:59291 failed: ... connect timeout after 8s`

## Scenario Matrix

### 1) Pre-activation HTLC rejection
- Status: **BLOCKED (multi-node prerequisite not satisfied)**
- Reason: trial nodes could not establish peer connectivity.

### 2) Post-activation funding acceptance (all nodes)
- Status: **BLOCKED**
- Reason: no cross-node chain progress due `peer_count=0` on both trial nodes.

### 3) Claim propagation + block inclusion
- Status: **BLOCKED**

### 4) Wrong-preimage rejection across nodes
- Status: **BLOCKED**

### 5) Refund rejection before timeout
- Status: **BLOCKED**

### 6) Refund success at/after timeout
- Status: **BLOCKED**

### 7) Node restart during lifecycle
- Status: **BLOCKED**

### 8) Mempool persistence/reload
- Status: **BLOCKED**

### 9) Reorg handling
- Status: **BLOCKED**

### 10) Legacy P2PKH flows post-activation
- Status: **BLOCKED**

## Root Blocker
- Cross-host trial P2P ports selected for isolation (`59291/59292`) are not reachable between `irium-vps` and `irium-eu` in current network policy.
- As a result, no multi-node peering, no block relay, and no distributed HTLC lifecycle validation could be completed.

## Required Unblock Action
1. Open bidirectional TCP between hosts for trial P2P ports (or provide two hosts with reachable trial ports).
2. Re-run this report’s topology with the same activation-gated setup.
3. Execute full scenario matrix and append txids/heights/RPC outputs.
