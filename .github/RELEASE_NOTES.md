## Summary

This release activates **PoAW-X** (Proof of Assigned Work, eXtended) consensus on Irium mainnet at **block height 50,000**. This is a **mandatory hard fork** — every node operator and miner must upgrade to **v1.9.119** before block 50,000, or their node will reject the new chain and fall out of consensus.

Below block 50,000, v1.9.119 validates the chain identically to prior releases; PoAW-X engages automatically at the activation height.

## What is PoAW-X

PoAW-X replaces "first to find the proof of work wins" with **VRF-based leader election**: for each block the chain cryptographically and verifiably assigns proposer rights via a Verifiable Random Function (ECVRF, RFC 9381). Sortition is bound to a per-height seed and the proposer's on-chain-registered key, so **hashrate is irrelevant to who wins each block** — CPU, GPU, and ASIC miners compete as equals. PoAW-X also adds anti-domination weighting, a distributed finality committee, and sybil-resistant proposer registration.

## Activation height

**Block 50,000.** Activation is a fixed consensus constant (`MAINNET_POAWX_ACTIVATION_HEIGHT = 50_000`). Estimate the date from the current block rate on any explorer — and **upgrade as soon as possible rather than wait.**

## What changes for miners

From block 50,000, **mining requires a full `iriumd` node running alongside your miner.** Pool-only stratum connections without a full node will not be able to participate in PoAW-X block production, because every block must carry a verifiable VRF proposer assignment and role receipts that only a full node can build and validate.

- Run `iriumd` v1.9.119.
- Mine against it with `irium-miner --poawx`.
- See **[docs/MINING.md](https://github.com/iriumlabs/irium/blob/main/docs/MINING.md)** and **[docs/POAWX.md](https://github.com/iriumlabs/irium/blob/main/docs/POAWX.md)**.

## Reward distribution

Each block reward is split automatically across four contribution roles via the multi-role reward system:

| Role | Share |
|------|-------|
| Proposer | 55% |
| Compute | 22% |
| Verify | 13% |
| Support | 10% |

In solo mining, one identity fills all four roles and receives the full reward.

## Security

Validated security properties of PoAW-X consensus:

- **VRF leader sortition** — ECVRF (RFC 9381); the proposer assignment proof verifies against the registered VRF key and per-height seed, and cannot be forged or predicted before the parent block.
- **Sybil-resistant proposer registration** — on-chain VRF-key registration with a **20-bit per-identity ticket proof-of-work cost**, frozen below the tip so the per-height seed cannot be used to register a winning key after the fact.
- **Distributed finality committee** — **2/3 supermajority** of distinct registered keys (**minimum 16 members** on mainnet).
- **Anti-domination engine** — per-identity proposal weighting over a **rolling 2016-block window**.
- **Hard reorg-depth cap** and **deterministic fork-choice tiebreaks**.

These were validated end-to-end in a **2,123-block, 3-node adversarial soak test with zero consensus errors** before mainnet activation was scheduled.

## Audit

The PoAW-X implementation underwent a full internal security audit that identified and fixed **15 critical/high-severity issues** plus **2 chain-split root causes** before mainnet activation.

## Upgrade instructions

1. Download the archive for your platform from the assets below, along with `checksums.txt`.
2. Verify the download:
   ```
   sha256sum -c checksums.txt
   ```
3. Extract, replace your `iriumd` binary, and restart:
   ```
   sudo systemctl restart iriumd
   ```
4. Confirm the version: `iriumd --version` reports `1.9.119`.
5. New to running a node? See **[QUICKSTART.md](https://github.com/iriumlabs/irium/blob/main/QUICKSTART.md)**.

**Upgrade before block 50,000** — older binaries will reject post-activation blocks.

### First start: one-time re-validation (expected)

The first time v1.9.119 starts on a node that already has chain history, it performs a **one-time re-validation** of its stored chain. Your node may briefly show a **lower block height and then climb back** to the network tip over a few minutes — this is **normal and expected**, happens only **once** (on the first start after upgrading), and **loses no data**. Let it finish; subsequent restarts are fast. This does not affect the height-gated activation at block 50,000. *(A later release will remove this one-time step.)*

## Assets & changelog

Platform binaries (Linux x86_64/arm64, macOS x86_64/arm64, Windows x86_64) and `checksums.txt` (SHA-256) are attached below. Full PoAW-X commit history: **[commit log up to v1.9.119](https://github.com/iriumlabs/irium/commits/v1.9.119)**.
