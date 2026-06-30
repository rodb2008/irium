## Summary

This release activates **PoAW-X** (Proof of Assigned Work, eXtended) consensus on Irium mainnet at **block height 50,000**. This is a **mandatory hard fork** — every node operator and miner must upgrade to **v1.9.119** before block 50,000, or their node will reject the new chain and fall out of consensus.

Below block 50,000, v1.9.119 validates the chain identically to prior releases; PoAW-X engages automatically at the activation height.

## What is PoAW-X

PoAW-X adds a block-proposer layer **on top of** Irium's existing **SHA-256d proof of work, which remains unchanged and fully merged-mining compatible**. For each block, an eligible proposer is selected by a Verifiable Random Function (ECVRF, RFC 9381), bound to a per-height seed and the proposer's on-chain-registered key, so the proposer assignment cannot be forged or predicted before the parent block. An **anti-domination engine** weights proposal rights over a rolling 2016-block window so that no single high-hashrate operator can dominate the chain. PoAW-X also adds a **distributed finality committee** and **sybil-resistant proposer registration**.

The result is fairer, harder-to-dominate block production — proposal rights are distributed verifiably rather than won purely by raw hashrate concentration — while SHA-256d proof of work and merged mining are fully preserved.

## Activation height

**Block 50,000.** As of this release the mainnet tip is around block 46,700, advancing at roughly one block per minute, so activation is approximately **two days away (around 2 July 2026)**. This is an estimate from the current block rate and can move with hashrate — **upgrade as soon as possible rather than wait.**

## What changes for miners

From block 50,000, **mining requires a full `iriumd` node running alongside your miner.** Pool-only stratum connections without a full node will not be able to participate in PoAW-X block production, because every block must carry a verifiable VRF proposer assignment and role receipts that only a full node can build and validate.

- Run `iriumd` v1.9.119.
- Mine against it with `irium-miner --poawx`.
- See **[docs/MINING.md](https://github.com/iriumlabs/irium/blob/main/docs/MINING.md)** and **[docs/POAWX.md](https://github.com/iriumlabs/irium/blob/main/docs/POAWX.md)**.

## Reward distribution

PoAW-X defines a four-role reward split as its canonical design:

| Role | Share |
|------|-------|
| Proposer | 55% |
| Compute | 22% |
| Verify | 13% |
| Support | 10% |

This is the design ratio. **At activation, a solo miner fills all four roles and receives the full block reward.** Automatic distribution to separate compute, verify, and support contributors is a future capability that remains gated pending a governance decision.

## Security

Security properties of PoAW-X consensus:

- **VRF leader sortition** — ECVRF (RFC 9381); the proposer assignment proof verifies against the registered VRF key and per-height seed, and cannot be forged or predicted before the parent block.
- **Sybil-resistant proposer registration** — on-chain VRF-key registration with a **20-bit per-identity ticket proof-of-work cost**, frozen below the tip so the per-height seed cannot be used to register a winning key after the fact.
- **Distributed finality committee** — **2/3 supermajority** of distinct registered keys (**minimum 16 members** on mainnet).
- **Anti-domination engine** — per-identity proposal weighting over a **rolling 2016-block window**.
- **Hard reorg-depth cap** and **deterministic fork-choice tiebreaks**.

These were exercised in a multi-block, multi-node adversarial soak with zero consensus errors before mainnet activation was scheduled.

## Audit

PoAW-X underwent an **internal security review and a multi-item hardening pass**, and the two chain-split root causes found during pre-mainnet testing were fixed. The internal review reported no critical- or high-severity findings. **PoAW-X has not yet been independently audited;** an external audit is planned.

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

The first time v1.9.119 starts on a node that already has chain history, it performs a **one-time re-validation** of its stored chain. Your node may briefly show a **lower block height and then climb back** to the network tip over a few minutes — this is **normal and expected**, happens only **once** (on the first start after upgrading), and **loses no data**. Let it finish; subsequent restarts are fast. This does not affect the height-gated activation at block 50,000.

## Assets & changelog

Platform binaries (Linux x86_64/arm64, macOS x86_64/arm64, Windows x86_64) and `checksums.txt` (SHA-256) are attached below. Full PoAW-X commit history: **[commit log up to v1.9.119](https://github.com/iriumlabs/irium/commits/v1.9.119)**.
