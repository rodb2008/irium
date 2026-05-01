# Irium Stratum Source Snapshot

This directory tracks the Stratum pool source currently deployed on EU pool host.

Key behavior included:
- BIP34-compatible coinbase height encoding toggle for miner compatibility.
- Runtime toggle: `IRIUM_STRATUM_COINBASE_BIP34`.
- Default in this snapshot: enabled (`true`) to avoid legacy miner height parsing issues.
- Local metrics and health endpoint (`/metrics`, `/health`) for explorer telemetry.

Note:
- This is an operational Stratum component used by pool infrastructure.
- Deploy target path on hosts: `/opt/irium-pool/irium-stratum`.
