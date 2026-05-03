# irium-js

JavaScript/TypeScript SDK for the [Irium](https://iriumlabs.org) settlement blockchain.

Covers the complete settlement lifecycle: offers, agreements, proofs, and real-time events.

## Install

```bash
npm install irium-js
```

## Quick start

```typescript
import { IriumClient, formatIrm } from "irium-js";

const client = new IriumClient({
  nodeUrl: "http://your-node:38300",
  token: "your-rpc-token", // optional; required if IRIUM_RPC_TOKEN is set on the node
});

const status = await client.getStatus();
console.log(`Height: ${status.height}, Peers: ${status.peer_count}`);

const balance = await client.getBalance("Q...");
console.log(`Balance: ${formatIrm(balance.balance)} IRM`);
```

## API reference

### `IriumClient`

Constructor: `new IriumClient({ nodeUrl, token? })`

| Method | Description |
|---|---|
| `getStatus()` | Node status — height, peers, genesis hash, network era |
| `getHeight()` | Current block height |
| `getBalance(address)` | Balance and UTXO count for an address |
| `getOffers(filters?)` | Offer feed with optional status/paymentMethod/amount filters |
| `computeAgreementHash(agreement)` | Compute the deterministic hash for an agreement object |
| `getAgreementStatus(agreement)` | Full lifecycle status for an agreement |
| `getReleaseEligibility(agreement)` | Check if a funded agreement is release-eligible |
| `getRefundEligibility(agreement)` | Check if a funded agreement is refund-eligible |
| `buildOtcTemplate(params)` | Build an OTC escrow policy template |
| `submitProof(proof)` | Submit a proof against an agreement |
| `listProofs(params)` | List all proofs for an agreement hash |
| `getProof(proofId)` | Retrieve a specific proof by ID |
| `checkPolicy(params)` | Check a policy against current proof state |
| `storePolicy(policy, replace?)` | Store a policy in the node's policy store |
| `getPolicy(agreementHash)` | Retrieve a stored policy |
| `evaluatePolicy(params)` | Evaluate a policy and return the result |
| `getAgreementTimeline(agreement)` | Full event timeline for an agreement |
| `getAgreementAudit(agreement)` | Audit log for an agreement |
| `submitTx(txHex)` | Broadcast a signed raw transaction |
| `getBlock(height)` | Block by height |
| `getBlockByHash(hash)` | Block by hash |
| `getTx(txid)` | Transaction by txid |
| `getNetworkHashrate()` | Current network hashrate |
| `getFeeEstimate()` | Recommended fee rate |
| `subscribeEvents(eventTypes, handler, filter?)` | Subscribe to real-time WebSocket events |
| `closeEvents()` | Close the WebSocket connection |

### Offer filters

```typescript
await client.getOffers({
  status: "open",          // "open" | "taken" | "filled" | "expired"
  paymentMethod: "bank-transfer",
  minAmount: 50_000_000,   // satoshis
  maxAmount: 500_000_000,
});
```

### Real-time events

```typescript
const ws = client.subscribeEvents(
  ["agreement.satisfied", "block.new"],
  (event) => console.log(event),
  { agreement_hash: "abc123..." }, // optional filter
);

// Later:
client.closeEvents();
```

Supported event types:
- `agreement.funded`
- `agreement.proof_submitted`
- `agreement.satisfied`
- `agreement.timeout`
- `agreement.disputed`
- `agreement.proof_reorged`
- `proof.gossip_received`
- `offer.created`
- `offer.taken`
- `block.new`
- `peer.connected`
- `peer.disconnected`

### Utilities

```typescript
import { formatIrm, parseIrm, isValidAddress, satoshisToIrm, irmToSatoshis } from "irium-js";

formatIrm(100_000_000)     // "1.00000000"
parseIrm("1.0")            // 100_000_000
isValidAddress("Qabc...")  // true/false
```

## Settlement lifecycle example

```typescript
// 1. Browse offers
const offers = await client.getOffers({ status: "open" });

// 2. Compute agreement hash (both parties must agree on the same object)
const { agreement_hash } = await client.computeAgreementHash({
  seller: { pubkey: "03..." },
  buyer:  { pubkey: "02..." },
  amount_irm: 100_000_000,
});

// 3. Build policy template (seller creates the escrow policy)
const template = await client.buildOtcTemplate({
  policy_id: "my-trade-001",
  agreement_hash,
  attestors: [{ pubkey: "03..." }],
  release_proof_type: "service_completion",
  refund_deadline_height: await client.getHeight() + 1000,
});

// 4. Check agreement status
const status = await client.getAgreementStatus({ seller: {...}, buyer: {...}, amount_irm: 100_000_000 });

// 5. Submit proof (when service is delivered)
await client.submitProof({
  agreement_hash,
  proof_type: "service_completion",
  payload: { /* proof payload */ },
  attestor_pubkey: "03...",
  attestor_signature: "...",
});

// 6. Check release eligibility
const eligible = await client.getReleaseEligibility({ ... });
if (eligible.eligible) {
  // Submit release_tx_hex via submitTx
  await client.submitTx(eligible.release_tx_hex);
}
```

## Building

```bash
npm install
npm run build
```

Output goes to `dist/`.

## Notes

- Offer creation and transaction signing require the `irium-wallet` CLI — the SDK does not hold private keys.
- Reputation data is wallet-local; use `irium-wallet reputation-show` to query it.
- WebSocket connections auto-reconnect with exponential backoff.
- All amounts are in satoshis (1 IRM = 100,000,000 satoshis).

## Licence

MIT
