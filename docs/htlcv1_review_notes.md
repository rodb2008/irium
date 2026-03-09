# HTLCv1 Review Notes

Scope:
- tx serialization compatibility
- sighash behavior for claim/refund
- mempool vs block validation consistency
- activation boundary behavior at N-1 / N / N+1
- unintended legacy non-HTLC changes

Findings:

1) Serialization compatibility
- Outer tx format unchanged.
- HTLCv1 encoded only in script_pubkey/script_sig payloads.
- Legacy P2PKH txid path unchanged.

2) Signature hash behavior
- Claim/refund use existing signature_digest path with HTLC script as scriptCode.
- Same low-S and pubkey verification policy as legacy path.

3) Mempool vs block validation consistency
- Admission paths call chain fee/validation checks (calculate_fees).
- HTLC activation and witness validity checks are enforced consistently.

4) Activation boundary
- height < activation: HTLC funding/spend rejected.
- height >= activation: HTLC allowed if valid.
- Covered by tests including pre/post activation checks.

5) Legacy path stability
- Legacy P2PKH regression tests pass.
- decodehtlc non-HTLC path returns p2pkh/unknown without changing legacy tx behavior.

Notes:
- inspecthtlc currently reports state from UTXO presence and timeout checks; it does not yet infer historical spend path metadata.
- RPC integration tests are in-process handler tests with deterministic chain-state application for mined-state simulation.

Conclusion:
- No consensus regression proven in covered scenarios.
- Safe for controlled devnet/testnet trial only.
- Mainnet activation must remain off pending explicit rollout decision.
