# PoAW-X auditor checklist

A working checklist for the external auditor. Each item lists where to look. Mark
pass/fail/finding.

## Cryptographic correctness
- [ ] `vrf_fun`/`secp256kfun` ECVRF (RFC 9381) implemented correctly, incl. vendored k256
      field arithmetic (`secp256kfun/src/vendor/k256`). [pre-1.0; primary target]
- [ ] Constant-time where required (`subtle-ng` usage); no secret-dependent branches.
- [ ] `bincode` `VrfProof` encoding is strictly canonical (no alternate/short/long encodings).

## VRF proof verification — `src/poawx_candidate.rs`
- [ ] `vrf_message` binds domain, network_id, target_height, role_id, solver_pkh,
      ticket_digest, seed, assignment_public_key.
- [ ] `validate` checks version/net/height/digest, parses the key, decodes the proof
      (exact length), `tai::verify`, and `output == vrf_output`.
- [ ] score = first-8-bytes-LE of the VRF output; no bias/grindability concern.

## Replay / substitution resistance
- [ ] A proof cannot be reused across height / role / miner / ticket / seed / key.

## Reward accounting — `src/poawx.rs`
- [ ] 55/22/13/10 split (bps 5500/2200/1300/1000 = 10000); remainder → PRIMARY.
- [ ] Official fee-0; third-party fee ≤ 2.00% (200 bps); invalid terms fail closed to 0%.
- [ ] No value created/destroyed; delegate/fee never double-credited.

## Candidate omission / addition — `src/poawx_admission.rs`, `src/chain.rs`
- [ ] Block candidate set must EQUAL the node's admitted set under enforcement.
- [ ] Selected role solver is the best admitted candidate.

## Committed admission — `src/poawx_committed_admission.rs`
- [ ] Root commits the next height's admitted set in block H-1; freeze seed = grandparent.
- [ ] Root binds candidate digests (and thus VRF outputs under V2); reorg-safe.

## Dominance reorg correctness — `src/poawx_dominance.rs`, `src/chain.rs`
- [ ] apply-on-connect / revert-on-disconnect symmetry; validated vs persisted parent state;
      restart-replay correct.

## Finality threshold / signatures — `src/poawx_finality.rs`
- [ ] Member-signed secp256k1 votes verified; committee membership + threshold enforced;
      finalizes the parent (no circularity); pool only bundles.

## Puzzle proof verification — `src/poawx_puzzle.rs`
- [ ] Challenge recomputed from the selected candidate; fast bounded verify; integer-only;
      no interaction with chain difficulty/LWMA.

## Private-key leakage — `src/bin/irium-wallet.rs`
- [ ] Secrets are input-only; never in JSON/logs/errors; submit posts only public wire.
- [ ] Pool holds no VRF secret; never proves (production code).

## Mainnet hard-off — all gates
- [ ] `network_id == 0` disables every gate; default-off; activation needs network≠0 +
      activation height + `*_REQUIRED=1`; no bypass path.

## Serialization bounds — all deserialize paths
- [ ] Fixed `*_WIRE` sizes; `*_MAX_BYTES`/`*_CAP` caps; exact-length checks; unknown/duplicate
      magic rejected; no panics on attacker input (`Result` errors, no `unwrap`/`expect` on
      untrusted bytes in non-test consensus paths).

## Denial-of-service vectors
- [ ] Gossip caches: validate→window→dedupe→bounded store; rebroadcast only newly-accepted;
      payload size caps; pruning by height.
- [ ] Loopback-only RPC bridges; no off-loopback reach; testnet/devnet only.

## Pool / node / wallet trust boundaries
- [ ] Node is authoritative and re-validates everything; pool/wallet are untrusted producers;
      no path lets a producer bypass node validation.

## Determinism
- [ ] No floats in consensus (lone `f64` is `#[cfg(test)]`); no wall-clock; canonical ordering
      (`BTreeMap`/`BTreeSet` + explicit sorts); domain separators on every digest.
