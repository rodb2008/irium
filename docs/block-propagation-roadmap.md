# Block Propagation Roadmap

## Current Relay Behavior
- New blocks accepted through the HTTP submit path are broadcast to connected peers.
- P2P sync already supports `getheaders`, `headers`, `getblocks`, and full `block` transfer.
- Incoming blocks still follow the existing validation path; no acceptance rules were changed.

## Groundwork Added In This Change
- Header-first announcement preference for locally accepted fresh-tip blocks.
- Full-block relay fallback retained for backward compatibility.
- Short-lived bounded caches for recent block relays and recent block requests.
- Low-noise propagation telemetry for operators via `/metrics`.

## Intentionally Deferred
- Compact block protocol.
- Transaction prefetch or mempool reconciliation.
- Any wire-format or consensus changes.

## Future Direction
- Compact block design using the existing header-first relay groundwork.
- Smarter block body fetch parallelism after header acceptance.
- Mempool-aware relay optimization once tx inventory and reconciliation are stronger.
