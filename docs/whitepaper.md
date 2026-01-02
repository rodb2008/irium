# Irium: DNS-Free Proof-of-Work Mainnet

**Technical Whitepaper — Version 1.0**

**Network Status:** LIVE on Mainnet  
**Genesis Hash:** 000000001f83c27ca5f3447e75a00ef1c66966af157fc12a823675b897f2fd6c  
**Genesis File:** `configs/genesis-locked.json` (matched by `bootstrap/anchors.json`)  
**Launch Date:** January 1, 2025 (genesis timestamp: 1735689600)

---

## Abstract

Irium is a purpose-built proof-of-work blockchain designed to maximize network independence and long-term survivability. The protocol eliminates reliance on DNS infrastructure, enforces transparent founder vesting via on-chain timelocks, incentivizes transaction relay quality without inflation, and prioritizes light-client usability from genesis. This whitepaper outlines the core design principles, system architecture, monetary policy, and implementation status of the IRM asset and its supporting network.

**Current Implementation:** Production mainnet is LIVE; DNS-free bootstrap and anchor-verified sync are enforced in the shipped Rust node.

---

## 1. Introduction

Most established proof-of-work networks inherit architectural assumptions from Bitcoin, including DNS-based bootstrapping, addrman-driven peer discovery, and an absence of protocol-level incentives for fast relay. Irium rethinks these components to produce a mainnet that can launch and sustain itself even if all founding infrastructure disappears.

**Irium launched with:**
- A mined genesis block at Bitcoin-standard difficulty bits (0x1d00ffff)
- Zero DNS dependencies (seedlist + anchors only)
- Anchor-verified sync and sybil-resistant P2P handshake
- Rust full node, miner, and SPV client

### 1.1 Goals

1. **Permanent bootstrap viability** without DNS dependencies or trusted third-party domains  
2. **Transparent founder vesting** with consensus-enforced, irreversible timelocks  
3. **Incentivized relay network** with optional fee-sharing commitments for propagation quality  
4. **Sybil-resistant peer discovery** hardened against botnet saturation  
5. **Mobile-first architecture** with SPV-ready light clients from block 1 and NiPoPoW on the roadmap  
6. **On-chain notarization** layer for off-chain metadata commitments  

### 1.2 Technical Specifications

| Parameter | Value |
|-----------|-------|
| Ticker | IRM |
| Algorithm | SHA-256d (Bitcoin-compatible) |
| Max Supply | 100,000,000 IRM |
| Genesis Vesting | 3,500,000 IRM (3.5%) |
| Mineable Supply | 96,500,000 IRM (96.5%) |
| Block Time | 600 seconds (10 minutes) |
| Initial Reward | 50 IRM |
| Halving Interval | 210,000 blocks (~4 years) |
| Difficulty Retarget | 2016 blocks (~14 days) |
| Coinbase Maturity | 100 blocks |
| P2P Port | 38291 |
| HTTP API (default) | 127.0.0.1:38300 |

---

## 2. System Architecture

Irium separates responsibilities into modular subsystems.

### 2.1 Core Rust Modules (`src/`)

- `block.rs` — Block and BlockHeader structures  
- `chain.rs` — ChainState, ChainParams, consensus validation, anchor enforcement  
- `tx.rs` — Transaction model and encoding/decoding  
- `wallet.rs` — Key utilities and address/script helpers  
- `pow.rs` — SHA-256d hashing, target difficulty, compact bits  
- `protocol.rs` — P2P binary message protocol and limits  
- `p2p.rs` — P2P node with peer management and header-first sync  
- `mempool.rs` — Transaction pool with fee-per-byte prioritization and eviction  
- `anchors.rs` — Anchor checkpoints loader/validator  
- `sybil.rs` — Sybil-resistant handshake helper  
- `spv.rs` — Header-only chain and merkle proof verification  

### 2.2 Executables

- `iriumd` — Full node with HTTP API and P2P networking  
- `irium-miner` — PoW miner with block broadcasting and anchor digest reporting  
- `irium-spv` — Header-first SPV client utilities  

### 2.3 Bootstrap & Configuration

- `configs/genesis-locked.json` — Locked genesis used by the node and miner  
- `bootstrap/anchors.json` — Signed anchors for eclipse protection  
- `bootstrap/seedlist.txt` — DNS-free seed IPs (shipped with signatures)  
- `bootstrap/seedlist.runtime` — Runtime peer cache saved locally  
- `configs/node.json` — Optional node configuration (P2P bind/seed overrides, relay address)  
- `scripts/irium-zero.sh` — DNS-free bootstrap helper  
- `systemd/iriumd.service` and `systemd/irium-miner.service` — systemd unit templates  

---

## 3. Consensus Mechanics

### 3.1 Proof-of-Work (SHA-256d)

Irium uses double SHA-256 hashing, identical to Bitcoin:

```
block_hash = SHA256(SHA256(header))
valid_block = block_hash < target
```

**Genesis Block (from `configs/genesis-locked.json`):**
- Hash: 000000001f83c27ca5f3447e75a00ef1c66966af157fc12a823675b897f2fd6c  
- Merkle Root: cd78279c389b6f2f0a4edc567f3ba67b27daed60ab014342bb4a5b56c2ebb4db  
- Nonce: 1,364,084,797  
- Bits: 0x1d00ffff  
- Timestamp: 1735689600 (January 1, 2025)  
- Anchored in `bootstrap/anchors.json` (signed by `iriumlabs`)  

### 3.2 Difficulty Adjustment

**Target:** 600 seconds per block  
**Retarget Interval:** Every 2016 blocks (~14 days)  

**Algorithm:**
```text
expected_time = 2016 * 600  # 1,209,600 seconds
actual_time = last_block_time - first_block_time
new_difficulty = old_difficulty * (actual_time / expected_time)
```

**Compact Target (Bitcoin-standard):**
```text
def to_target(bits: int) -> int:
    exponent = bits >> 24
    mantissa = bits & 0xFFFFFF
    if exponent <= 3:
        return mantissa >> (8 * (3 - exponent))
    else:
        return mantissa << (8 * (exponent - 3))
```

### 3.3 Block Validation

Each block must satisfy:  
1. `block_hash < target` (proof-of-work)  
2. Merkle root matches transaction tree  
3. Timestamp within consensus range  
4. All transactions valid (no double-spends)  
5. Coinbase reward ≤ subsidy + fees  
6. Block connects to a valid chain tip  
7. If anchors are loaded, the block hash at each anchored height must match the signed anchor  

---

## 4. Economic Model

### 4.1 Supply Distribution

**Total Supply:** 100,000,000 IRM (hard cap)  

**Genesis Vesting (3.5%):** 3,500,000 IRM (CLTV-locked in genesis)  
- 1,000,000 IRM unlocks at block 52,560 (~1 year)  
- 1,250,000 IRM unlocks at block 105,120 (~2 years)  
- 1,250,000 IRM unlocks at block 157,680 (~3 years)  

**Mineable Supply (96.5%):** 96,500,000 IRM  
- Distributed via block rewards  
- Halves every 210,000 blocks  

### 4.2 Block Rewards Schedule

| Block Range | Reward | Blocks | Total IRM |
|-------------|--------|--------|-----------|
| 1 - 210,000 | 50 IRM | 210,000 | 10,500,000 |
| 210,001 - 420,000 | 25 IRM | 210,000 | 5,250,000 |
| 420,001 - 630,000 | 12.5 IRM | 210,000 | 2,625,000 |
| 630,001 - 840,000 | 6.25 IRM | 210,000 | 1,312,500 |
| ... | Continues halving | ... | ... |

### 4.3 Transaction Fees

**Fee Handling:** Fees are fully paid to the miner by default; relay commitments are supported via explicit coinbase outputs without inflating supply. The mempool enforces a minimum fee-per-byte floor (configurable; defaults to 1.0 unit/byte) and evicts lowest-fee entries when full.

---

## 5. The 8 Core Innovations

### 5.1 Zero-DNS Bootstrap

- Signed `seedlist.txt` with raw IP multiaddrs (IPv4)  
- Signed `anchors.json` with checkpoint block headers  
- Bootstrap script `scripts/irium-zero.sh` (no DNS queries)  
- Distribution via Git, IPFS, or any file transport  
- Verification with `ssh-keygen -Y verify` against `bootstrap/trust/allowed_signers`  

**Status:** Implemented and used by default.

### 5.2 Self-Healing Peer Discovery

- Runtime peer cache `bootstrap/seedlist.runtime` persists discovered peers  
- Peer scoring with per-peer state in the P2P layer (promotion/demotion based on outcomes)  
- Outbound dialer periodically connects to learned peers (not just static seeds) to grow the mesh, even behind NAT  
- Works without centralized trackers  

**Status:** Implemented; scoring and dial cadence continue to be iterated in Rust.

### 5.3 Genesis Vesting with On-chain CLTV

- 3.5M IRM locked at genesis via timelocked outputs  
- Unlock heights: 52,560 / 105,120 / 157,680 blocks  
- Consensus-enforced and visible in `configs/genesis-locked.json`  

**Status:** Implemented in genesis; enforced by validation.

### 5.4 Per-Transaction Relay Rewards (Opt-In)

- Relay commitments can be embedded in coinbase transactions (50/30/20 fee split supported in tooling)  
- No inflation; rewards are sourced from transaction fees  
- Relay memo support via OP_RETURN for auditability  

**Status:** Commitment construction supported in `relay.rs`; miners choose when to include.

### 5.5 Sybil-Resistant P2P Handshake

- Proof-of-work challenge during handshake (configurable difficulty)  
- Timestamp validation (5-minute window)  
- Ephemeral key signing  

**Status:** Implemented (`sybil.rs`, enforced in `p2p.rs`).

### 5.6 Anchor-File Consensus

- Signed checkpoint headers in `bootstrap/anchors.json`  
- Multiple signer support; digest reporting in node/miner logs  
- Nodes reject chains that diverge from shipped anchors at anchored heights  

**Status:** Implemented and enforced when anchors are present.

### 5.7 Light Client First (SPV; NiPoPoW Roadmap)

- Header-only sync with PoW verification (`spv.rs`)  
- Merkle proof verification for transactions  
- NiPoPoW proofs planned for ultra-light clients  

**Status:** SPV implemented; NiPoPoW planned.

### 5.8 On-chain Metadata Commitments

- Coinbase metadata (OP_RETURN) for notarization of off-chain data  
- Relay memo support for provenance of propagation rewards  

**Status:** Structure supported in transaction builder utilities.

---

## 6. Consensus Implementation

### 6.1 Block Structure

**Header (80 bytes):**
```
Version (4 bytes)
Previous Hash (32 bytes)
Merkle Root (32 bytes)
Time (4 bytes)
Bits (4 bytes)
Nonce (4 bytes)
```

**Block:**
- Header (80 bytes)  
- Transaction count (varint)  
- Transactions (variable)  

### 6.2 Transaction Structure

- UTXO model (Bitcoin-like)  
- Inputs reference previous outputs; outputs create spendable amounts  
- Scripts are standard P2PKH; OP_RETURN used for commitments  

### 6.3 Merkle Tree

- Transactions organized in a merkle tree  
- Root committed in the block header  
- Supports SPV merkle proof verification  

---

## 7. Network Protocol

### 7.1 P2P Binary Protocol

**Message Format:** `[Version:1][Type:1][Length:4][Payload:N]`  

**Active Message Types (Rust node):**  
Handshake (1), Ping (2), Pong (3), GetPeers (4), Peers (5), GetBlocks (6), Block (7), GetHeaders (8), Headers (9), Tx (10), Mempool (11), SybilChallenge (12), SybilProof (13), Disconnect (99). Inv/GetData types are reserved for future expansion.

**Limits:** 32 MB max message size, 4 MB max block size.

### 7.2 Peer Management

- Default port: 38291  
- Max peers: tuned for production (see `p2p.rs`)  
- Ping interval: 60 seconds (public), 30 seconds (NAT)  
- Timeouts: 180 seconds for peers/messages; cleanup every 30 seconds  
- NAT support via outbound connections; self-connection detection and IP:PORT deduplication  
- Rate limits enforced for mempool admission and message sizes  

---

## 8. Security Analysis

### 8.1 Consensus Security

- **51% Attack:** Requires majority hashpower; anchored checkpoints provide rapid detection.  
- **Double-Spend:** Prevented by UTXO validation and chain reorg rules.  
- **Long-Range Attack:** Mitigated by signed anchors; new nodes verify against anchors before syncing.  

### 8.2 Network Security

- **Eclipse Attack:** Anchor enforcement plus peer banning and multiple seed sources.  
- **Sybil Attack:** PoW handshake and peer scoring; connection limits per node.  
- **DoS Attack:** Message size limits, mempool caps (default 1,000 entries), rate-limited submissions.  

### 8.3 Wallet Security

- secp256k1 keys, WIF format, local/non-custodial storage  
- Users retain backup responsibility  

---

## 9. Implementation Status

### 9.1 Completed

- Locked genesis loader and anchor enforcement  
- SHA-256d PoW validation and retargeting  
- UTXO tracking, block/transaction validation, coinbase maturity  
- P2P protocol with sybil challenge/response and header-first sync  
- Mempool with fee-per-byte ranking and eviction  
- SPV header chain + merkle proof verification  
- DNS-free bootstrap artifacts (seedlist + anchors + signatures)  
- Binaries: `iriumd`, `irium-miner`, `irium-spv` (release builds)  
- Ops tooling: systemd example, zero-DNS bootstrap script, auto-update script  

### 9.2 In Progress / Roadmap

- NiPoPoW proofs for ultra-light clients  
- Expanded relay reward automation (multi-relay payouts by default policy)  
- Additional public seeds and signer keys for anchors  
- Wallet UX (mobile/native) and hardware wallet support  
- Web explorer UI (API is live)  

### 9.3 Network Launch

- **Mainnet:** LIVE  
- **Bootstrap seeds:** `bootstrap/seedlist.txt` (signed) + `bootstrap/seedlist.extra` (unsigned additions)  
- **Anchors:** `bootstrap/anchors.json` (signed)  
- **APIs:**  
  - Explorer API: self-hosted via `/api`  
  - Wallet API: self-hosted via `/wallet`  

---

## 10. Governance

Irium follows rough consensus and public review.

**Upgrade Process:**
1. Draft improvement proposal  
2. Community review and discussion  
3. Prototype implementation  
4. Multi-stakeholder audit  
5. Network upgrade via miner signaling  

No on-chain governance; decisions are made through code and consensus.

---

## 11. Future Roadmap

- [x] Core blockchain implementation  
- [x] PoW mining system  
- [x] P2P networking with sybil defense  
- [x] DNS-free bootstrap (seedlist + anchors)  
- [x] Mainnet genesis and launch  
- [ ] NiPoPoW proofs  
- [ ] Expanded relay payout automation  
- [ ] Mobile wallet app  
- [ ] Mining pool software  
- [ ] Hardware wallet integration  
- [ ] Exchange listings  
- [ ] Block explorer web UI  

---

## 12. Conclusion

Irium addresses fundamental challenges in decentralization, security, and accessibility by removing DNS dependencies, enforcing transparent vesting, and prioritizing light clients from block 1. With anchor-verified sync, sybil-resistant networking, and production Rust tooling, Irium is positioned as a resilient proof-of-work mainnet built for long-term survivability.

**Network Status:** Mainnet LIVE and operational  
**Code Status:** Production-ready Rust implementation (open source)  
**Community:** Contributions welcome  

---

## 13. References

1. Satoshi Nakamoto, "Bitcoin: A Peer-to-Peer Electronic Cash System," 2008  
2. Aggelos Kiayias et al., "Non-Interactive Proofs of Proof-of-Work (NiPoPoWs)," 2017  
3. libp2p Project, "libp2p Specification," https://github.com/libp2p/specs  
4. BIP-34: Block Height in Coinbase  
5. BIP-65: OP_CHECKLOCKTIMEVERIFY  

---

## Appendix A: Genesis Block (Mainnet)

- File: `configs/genesis-locked.json`  
- Hash: 000000001f83c27ca5f3447e75a00ef1c66966af157fc12a823675b897f2fd6c  
- Merkle Root: cd78279c389b6f2f0a4edc567f3ba67b27daed60ab014342bb4a5b56c2ebb4db  
- Nonce: 1,364,084,797  
- Bits: 0x1d00ffff  
- Timestamp: 1735689600 (January 1, 2025)  
- Anchors: `bootstrap/anchors.json` (signed)  

---

**Irium Blockchain © 2025**  
**MIT License — Open Source**

*Built for true decentralization*
