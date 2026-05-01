# Developer Quick Reference

Commands only. No explanation. Default RPC: `http://localhost:38300`.

---

## 1. Check Node Is Synced

```bash
curl http://localhost:38300/status
```

Expected: `"persisted_height"` equals `"height"` and `"peer_count"` > 0.

```bash
# Compact check
curl -s http://localhost:38300/status | python3 -m json.tool | grep -E '"height"|"peer_count"'
```

---

## 2. Query Balance for an Address

```bash
curl "http://localhost:38300/rpc/balance?address=Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa"
```

List UTXOs:
```bash
curl "http://localhost:38300/rpc/utxos?address=Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa"
```

---

## 3. Get Block by Height and by Hash

By height:
```bash
curl "http://localhost:38300/rpc/block?height=20296"
```

By hash:
```bash
curl "http://localhost:38300/rpc/block_by_hash?hash=000000000697c1d50667fbde625d93dbc172f915021c63d42bd79abbde0f5fed"
```

---

## 4. Look Up a Transaction

```bash
curl "http://localhost:38300/rpc/tx?txid=17edd1b2363712e2f380ba6e10510f9ff3a2b45881433d718859d1bbb116293c"
```

---

## 5. Get Network Hashrate

```bash
curl http://localhost:38300/rpc/network_hashrate
```

---

## 6. Create and Broadcast a Transaction

```bash
# Check balance first
irium-wallet balance Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa

# Check fee estimate
irium-wallet estimate-fee

# Send
irium-wallet send Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa QDestinationAddress... 1.0

# Send with explicit fee
irium-wallet send Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa QDestinationAddress... 1.0 --fee 0.0001
```

---

## 7. Create an OTC Offer

```bash
irium-wallet offer-create \
  --seller Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa \
  --amount 1.0 \
  --payment-method bank-transfer \
  --timeout 25000 \
  --price-note "1 IRM = 0.10 USD" \
  --payment-instructions "Send to IBAN DE89..."
```

---

## 8. List Open Offers from the Feed

```bash
curl http://localhost:38300/offers/feed
```

Filter with CLI:
```bash
irium-wallet offer-list --status open --sort newest --limit 20
```

---

## 9. Take an Offer

```bash
irium-wallet offer-take \
  --offer d1-gossip-t4 \
  --buyer QBuyerAddress... \
  --rpc http://localhost:38300
```

---

## 10. Check Agreement Status

```bash
irium-wallet agreement-status agreement.json --rpc http://localhost:38300
```

Via RPC:
```bash
curl -X POST http://localhost:38300/rpc/agreementstatus \
  -H "Content-Type: application/json" \
  -d '{"agreement_hash": "<agreement_hash_hex>"}'
```

Check release eligibility:
```bash
irium-wallet agreement-release-eligibility agreement.json --rpc http://localhost:38300
```

Check refund eligibility:
```bash
irium-wallet agreement-refund-eligibility agreement.json --rpc http://localhost:38300
```

---

## 11. Submit a Proof

```bash
# Create proof
irium-wallet agreement-proof-create \
  --agreement-hash <agreement_hash_hex> \
  --proof-type delivery_confirmed \
  --attested-by attestor-id \
  --address QAttestorAddress... \
  --evidence-summary "Delivery confirmed" \
  --out proof.json

# Submit proof
irium-wallet agreement-proof-submit --proof proof.json --rpc http://localhost:38300
```

Via RPC:
```bash
curl -X POST http://localhost:38300/rpc/submitproof \
  -H "Content-Type: application/json" \
  -d @proof.json
```

List submitted proofs:
```bash
curl -X POST http://localhost:38300/rpc/listproofs \
  -H "Content-Type: application/json" \
  -d '{"agreement_hash": "<agreement_hash_hex>"}'
```

---

## 12. Check Release Eligibility

```bash
irium-wallet agreement-release-eligibility agreement.json --rpc http://localhost:38300
```

Via RPC:
```bash
curl -X POST http://localhost:38300/rpc/agreementreleaseeligibility \
  -H "Content-Type: application/json" \
  -d '{"agreement_hash": "<agreement_hash_hex>"}'
```

Release funds (requires secret preimage):
```bash
irium-wallet agreement-release agreement.json \
  --secret <secret_preimage_hex> \
  --broadcast \
  --rpc http://localhost:38300
```

Refund after timeout:
```bash
irium-wallet agreement-refund agreement.json \
  --broadcast \
  --rpc http://localhost:38300
```

---

## Useful One-Liners

```bash
# Current chain height
curl -s http://localhost:38300/status | python3 -c "import sys,json; print(json.load(sys.stdin)['height'])"

# Peer count
curl -s http://localhost:38300/status | python3 -c "import sys,json; print(json.load(sys.stdin)['peer_count'])"

# Network hashrate (H/s)
curl -s http://localhost:38300/rpc/network_hashrate | python3 -c "import sys,json; print(json.load(sys.stdin)['hashrate'])"

# Offer count in feed
curl -s http://localhost:38300/offers/feed | python3 -c "import sys,json; print(json.load(sys.stdin)['count'])"

# Balance in IRM (not satoshis)
curl -s "http://localhost:38300/rpc/balance?address=Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa" | \
  python3 -c "import sys,json; d=json.load(sys.stdin); print(d['balance']/1e8, 'IRM')"
```
