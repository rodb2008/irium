# Attestor Guide

Attestors are trusted third parties who sign proofs confirming that a trade condition
has been met. When an agreement requires proof of delivery, completion, or payment,
an attestor provides the signed evidence that releases settlement funds.

This guide covers attestor bonding — an economic accountability mechanism that gives
counterparties confidence that an attestor has real skin in the game.

---

## What is an Attestor Bond?

An attestor bond is an on-chain commitment recorded via OP_RETURN. When an attestor
registers a bond, they publish a signed transaction containing:

```
bond1:<pubkey_hash_hex>:<bond_atoms>
```

This anchor permanently records that the attestor has declared a bond of a specific
IRM amount. Counterparties can verify this record by scanning the chain or checking
`irium-wallet attestor-list` output.

**Why bonds matter:**
- Without a bond, an attestor has no economic accountability
- A bonded attestor risks reputational and economic harm if they attest fraudulently
- Agreements referencing unbonded attestors are accepted but flagged with a warning

---

## Slashing Conditions

An attestor's bond is considered slashed when two contradicting proofs signed by
the same attestor are submitted for the same agreement — one claiming the condition
is satisfied, another claiming it is unsatisfied.

When a slash is detected, any party can submit a slash record to the chain:

```
slash1:<attestor_pkh_hex>:<agreement_hash>
```

This creates a permanent on-chain record visible to all nodes. The local bond store
is updated to reflect the slash count and forfeited atoms.

Slashed funds go to the agreement's non-attesting party in the supported phase of
implementation.

---

## Commands

### Register a Bond

```bash
irium-wallet attestor-register --bond 10 [--from <address>] [--rpc <url>]
```

- `--bond` — IRM amount to declare as bonded (must be present in wallet)
- `--from` — wallet address to sign the registration tx (defaults to first key)
- `--rpc` — node RPC URL (defaults to `IRIUM_RPC_URL` or localhost)

Example output:
```
bond_registered
address          Q...
pkh_hex          a1b2c3...
bond_amount      10 IRM
registration_tx  deadbeef...
registered_at    height 20500
withdraw_eligible_after  height 21500
```

### Check Bond Status

```bash
irium-wallet attestor-bond-status [--address <address>] [--json] [--rpc <url>]
```

Shows all registered bonds on this node, or filters by address.

Example output:
```
address          Q...
bond_amount      10 IRM
status           active
registered_at    height 20500
registration_tx  deadbeef...
withdraw_eligible no (487 blocks remaining; eligible at height 21500)
```

### Withdraw a Bond

```bash
irium-wallet attestor-withdraw-bond [--from <address>] [--rpc <url>]
```

Withdraws the bond after the 1000-block cooldown period has passed. The cooldown
is measured from the later of the registration height and the last attestation height.

This command will fail if:
- No bond record exists for the address
- The bond is already withdrawn
- The cooldown period has not elapsed

### Record a Slash

```bash
irium-wallet attestor-slash \
  --attestor <attestor_address> \
  --proof1 <proof_id_1> \
  --proof2 <proof_id_2> \
  [--agreement <agreement_hash>] \
  [--rpc <url>]
```

Creates an on-chain slash record referencing the two contradicting proofs.
The local bond store is updated to reflect the slash.

---

## Viewing Bond Status in Attestor List

`irium-wallet attestor-list` shows bond status inline for each attestor:

```
attestors (2):
  alice-attestor (02abc...) — Alice [alice.example.com] [bond: 10 IRM active]
  bob-attestor (03def...) — Bob [bond: none — unbonded]
```

An `[unbonded]` attestor in a policy is always shown with a warning. Users should
require bonded attestors for high-value trades.

---

## Attestor Bond Lifecycle

```
attestor-register --bond 10
    → tx published with OP_RETURN bond1:<pkh>:1000000000
    → local store: registered_height=20500, bond_atoms=10 IRM

[attestor signs proofs over 1000+ blocks]

attestor-withdraw-bond
    → checks current_height >= max(20500, last_attest) + 1000
    → tx published with OP_RETURN bond1w:<pkh>
    → local store: withdrawn=true, withdraw_height=21600
```

---

## Cooldown Period

The 1000-block cooldown (approximately 16 hours at target block time) gives any party
time to submit a slash claim before the bond is withdrawn. Specifically:

- If the attestor has never signed a proof, cooldown starts from registration height
- If the attestor has signed proofs, cooldown starts from the most recent attestation

This ensures that a fraudulent attestor cannot register and immediately withdraw.

---

## Security Considerations

- The OP_RETURN bond record is permanent and public — anyone can verify it
- Bond declarations are social commitments, not cryptographic locks on specific UTXOs
- A higher bond amount signals greater economic accountability
- Counterparties should check attestor bond status before entering high-value agreements
- Nodes do not enforce bond minimum requirements; this is a user-level trust decision

---

## Anchor Format Reference

| Type | OP_RETURN payload |
|------|-------------------|
| Bond registration | `bond1:<pkh_hex_40>:<atoms_decimal>` |
| Bond withdrawal | `bond1w:<pkh_hex_40>` |
| Slash record | `slash1:<attestor_pkh_hex_40>:<agreement_hash_hex_64>` |

All payloads fit within the 75-byte OP_RETURN limit.
