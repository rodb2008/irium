# Key Derivation and Address Format

This document covers Irium's address format, key encoding, and the custom deterministic key derivation scheme used by the wallet.

---

## Address Format

Irium addresses use Base58Check encoding with a version byte of `0x39`.

**Encoding steps:**

1. Start with a 33-byte compressed secp256k1 public key.
2. Compute HASH160: `RIPEMD160(SHA256(compressed_pubkey))` — produces 20 bytes.
3. Prepend version byte `0x39` — total 21 bytes.
4. Compute checksum: first 4 bytes of `SHA256(SHA256(version || pkh))`.
5. Append checksum — total 25 bytes.
6. Base58Check-encode the 25 bytes.

The version byte `0x39` produces addresses starting with the letter `Q` on mainnet.

**Example:**
```
Public key (hex):  03e918af472e63de044c983df9f09bae57d4c78a70998d5d5fded408672886f868
HASH160 (hex):     79dbb6fd908884fc994b8aa34dcef392fe2d9d65
Version byte:      0x39
Address:           Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa
```

---

## Key Formats

### Private key

- Algorithm: secp256k1
- Length: 32 bytes
- Encoding: raw bytes, not further encoded outside of WIF export

### Public key

- Type: compressed secp256k1
- Length: 33 bytes (1-byte prefix `02` or `03` followed by 32-byte X coordinate)
- The prefix encodes the parity of the Y coordinate: `02` for even, `03` for odd

### Public key hash (PKH)

- `HASH160 = RIPEMD160(SHA256(compressed_pubkey))`
- Length: 20 bytes
- Used as the payload in P2PKH locking scripts and in address encoding

---

## WIF (Wallet Import Format)

WIF is used to export and import individual private keys.

**Encoding steps:**

1. Start with the 32-byte private key.
2. Prepend version byte `0x80` (same as Bitcoin mainnet WIF).
3. Append `0x01` to indicate a compressed public key.
4. Compute checksum: first 4 bytes of `SHA256(SHA256(version || key || flag))`.
5. Append checksum — total 38 bytes.
6. Base58Check-encode.

WIF-encoded Irium private keys begin with `5H`, `5J`, or `5K` (uncompressed) or `K` or `L` (compressed) — same range as Bitcoin due to the identical version byte.

---

## Key Derivation: Custom Deterministic Scheme

Irium does **not** use BIP32 or BIP39. The wallet uses a custom deterministic derivation scheme based on direct SHA256 hashing of the seed.

### Seed

- The seed is 32 bytes, hex-encoded as 64 characters.
- It is generated randomly at wallet initialisation, or imported explicitly.
- There is no BIP39 mnemonic phrase. The seed itself is the backup material.

### Deriving the key at index N

Given a 32-byte seed, the private key at index `N` is computed as:

```
for counter = 0 to 1023:
    data = seed_bytes || uint32_le(N) || uint32_le(counter)
    candidate = SHA256(data)
    if candidate is a valid secp256k1 scalar (non-zero, less than curve order):
        private_key = candidate
        break
```

Where `||` is byte concatenation and `uint32_le` is 4-byte little-endian encoding.

The counter loop handles the rare case where the SHA256 output is not a valid scalar. In practice the first iteration succeeds for the vast majority of inputs.

**Properties of this scheme:**

- Deterministic: the same seed and index always produce the same private key.
- Independent: key at index 0, 1, 2, ... are fully independent given knowledge of the seed.
- No parent/child hierarchy: there are no extended keys, no chain codes, no hardened derivation paths.
- Anyone with the seed can derive all addresses.

### Security implications

- The seed must be backed up securely. Loss of the seed means loss of all derived funds.
- There is no passphrase protection on the derivation itself. Encryption is applied at the wallet storage layer, not the derivation layer.
- The scheme is not compatible with BIP32/BIP44 hardware wallet derivation. Hardware wallet support would require implementing a separate BIP32 derivation path.

---

## Signing

- **Algorithm:** secp256k1 ECDSA
- **Digest:** SHA256d — `SHA256(SHA256(transaction_serialisation))`
- **Signature format:** DER-encoded with `SIGHASH_ALL` suffix byte `0x01`
- **Script type:** P2PKH (Pay to Public Key Hash)

Standard P2PKH locking script:
```
OP_DUP OP_HASH160 <20-byte-pkh> OP_EQUALVERIFY OP_CHECKSIG
```

Standard P2PKH unlocking script (scriptSig):
```
<DER signature + 0x01> <33-byte compressed pubkey>
```

---

## BIP44 Registration

Irium does **not** use BIP44 derivation. Because the wallet uses a custom non-hierarchical deterministic scheme, there is currently no BIP44 coin type to register.

If a future wallet implementation adds BIP32/BIP44-compatible derivation (for hardware wallet support or compatibility with other tooling), a coin type registration would be required. The correct registry is SLIP-0044, maintained at:

```
https://github.com/satoshilabs/slips/blob/master/slip-0044.md
```

The process is:

1. Check the current `slip-0044.md` file to confirm that no entry for IRM or Irium already exists. If another project has already registered `IRM` as a symbol, a different symbol or a note in the entry would be needed.
2. Fork the `satoshilabs/slips` repository on GitHub.
3. Add a new row to the table in `slip-0044.md` with:
   - **Index:** the next available decimal index
   - **Hex:** the index in `0x` hex format
   - **Symbol:** `IRM`
   - **Name:** `Irium`
   - **Link:** the Irium GitHub repository URL
4. Open a pull request against `satoshilabs/slips` with the single-line change.

Do not submit this pull request until a BIP32/BIP44-compatible wallet implementation exists and is publicly released. The SLIP-0044 maintainers require that the coin is actively used with BIP44 derivation before merging.

---

## Manual Address Derivation Example (Python pseudocode)

```python
import hashlib
import struct

def derive_private_key(seed_bytes: bytes, index: int) -> bytes:
    for counter in range(1024):
        data = seed_bytes + struct.pack('<I', index) + struct.pack('<I', counter)
        candidate = hashlib.sha256(data).digest()
        # Check if candidate is a valid secp256k1 scalar
        # (non-zero and less than the curve order)
        n = int.from_bytes(candidate, 'big')
        curve_order = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
        if 0 < n < curve_order:
            return candidate
    raise ValueError("Could not derive valid key (should not happen in practice)")

def pkh_from_pubkey(compressed_pubkey: bytes) -> bytes:
    sha = hashlib.sha256(compressed_pubkey).digest()
    ripemd = hashlib.new('ripemd160')
    ripemd.update(sha)
    return ripemd.digest()

def address_from_pkh(pkh: bytes) -> str:
    # Version byte 0x39 for Irium mainnet
    payload = bytes([0x39]) + pkh
    checksum = hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
    return base58check_encode(payload + checksum)
```

This pseudocode illustrates the derivation. A production implementation requires a correct secp256k1 library for public key computation.

---

## Key Derivation: BIP32/BIP39 Hierarchical Deterministic Scheme

Starting from Phase 2, Irium wallets can optionally use BIP32 hierarchical deterministic key derivation with BIP39 mnemonic phrases. This scheme is compatible with standard hardware wallets and third-party BIP32 tools.

The custom derivation scheme described above continues to work and is unchanged. BIP32 is an additional option for new wallets.

### BIP39 Mnemonic

- The wallet generates a 24-word English mnemonic phrase using the BIP39 standard wordlist.
- The seed is derived from the mnemonic using PBKDF2-HMAC-SHA512 with "mnemonic" as the salt and 2048 iterations, producing a 64-byte seed.
- No passphrase is applied by default (empty passphrase string).
- The mnemonic phrase is the complete backup material. Anyone with it can derive all wallet addresses.

### BIP32 Master Key

From the 64-byte BIP39 seed, the BIP32 master key is derived as:

```
I = HMAC-SHA512(key="Bitcoin seed", data=seed_bytes)
master_key        = I[0:32]   (left 32 bytes)
master_chain_code = I[32:64]  (right 32 bytes)
```

### Child Key Derivation

Child keys are derived using the standard BIP32 CKDpriv function:

```
For hardened child (index >= 0x80000000):
    I = HMAC-SHA512(key=chain_code, data=0x00 || parent_key || index_be)

For normal child (index < 0x80000000):
    I = HMAC-SHA512(key=chain_code, data=compressed_pubkey || index_be)

IL             = I[0:32]
chain_code_new = I[32:64]
child_key      = (IL + parent_key) mod n    (secp256k1 curve order n)
```

### Derivation Path

Irium uses the BIP44 path structure:

```
m/44'/1'/0'/0/<index>
```

| Level | Value | Description |
|-------|-------|-------------|
| Purpose | 44' | BIP44 multi-account hierarchy |
| Coin type | 1' | Coin type 1 (testnet / IRM pending BIP44 registration) |
| Account | 0' | Default account |
| Change | 0 | External (receiving) chain |
| Index | 0, 1, 2, ... | Individual address index |

**Note on coin type:** Coin type 1 is used until official SLIP-0044 registration for IRM. Once registered, the coin type will be updated. Wallets created before registration can be imported using the old path — they are not automatically migrated.

### Address Derivation

BIP32-derived keys produce Irium addresses using the same encoding as custom-scheme keys:

1. Derive the secp256k1 private key at the path above.
2. Compute the compressed public key (33 bytes).
3. `HASH160 = RIPEMD160(SHA256(compressed_pubkey))` — 20 bytes.
4. Prepend version byte `0x39` — 21 bytes.
5. Checksum: first 4 bytes of `SHA256(SHA256(version || pkh))`.
6. Base58Check-encode the 25 bytes.

This produces a standard Irium Q-address, indistinguishable in format from a custom-scheme address.

### Wallet Commands

| Command | Description |
|---------|-------------|
| `irium-wallet create-wallet --bip32` | Create BIP32/BIP39 wallet; prints 24-word mnemonic |
| `irium-wallet import-mnemonic "<24 words>"` | Restore BIP32 wallet from mnemonic |
| `irium-wallet create-wallet` | Create wallet using the custom derivation scheme |
| `irium-wallet new-address` | Derive next address (uses BIP32 path or custom, depending on wallet type) |

### Cross-Tool Verification

Any standard BIP32 tool using path `m/44'/1'/0'/0/0` with coin type 1 will derive the same private key and address from the same mnemonic.

Python verification example:
```python
import hmac, hashlib, struct

mnemonic = "<your 24 words>"
seed = hashlib.pbkdf2_hmac("sha512", mnemonic.encode(), b"mnemonic", 2048)
I = hmac.new(b"Bitcoin seed", seed, hashlib.sha512).digest()
# then apply CKDpriv for each level of m/44h/1h/0h/0/0
```

### Security Comparison

| Property | Custom scheme | BIP32/BIP39 |
|----------|--------------|-------------|
| Backup material | 64-char hex seed | 24-word English mnemonic |
| Hardware wallet support | No | Yes (once coin type registered) |
| Standard compatibility | No | Yes (BIP32, BIP44) |
| Key independence | Full | Full (hardened path levels) |
| Existing wallets affected | N/A | None — custom scheme unchanged |
