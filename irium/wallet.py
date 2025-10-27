"""Deterministic key management and transaction builder for Irium."""

from __future__ import annotations

import hashlib
import hmac
import secrets
from dataclasses import dataclass
from typing import Dict, Iterable, List, Optional, Tuple

from .constants import PUBKEY_ADDRESS_PREFIX
from .pow import sha256d
from .tx import Transaction, TxInput, TxOutput


# Secp256k1 domain parameters
_P = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F
_N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
_G = (
    55066263022277343669578718895168534326250603453777594175500187360389116729240,
    32670510020758816978083085130507043184471273380659243275938904335757337482424,
)

_BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def _int_to_bytes(value: int, length: int) -> bytes:
    return value.to_bytes(length, "big")


def _hash160(payload: bytes) -> bytes:
    """RIPEMD160(SHA256(data)) - with fallback"""
    sha = hashlib.sha256(payload).digest()
    try:
        ripemd = hashlib.new("ripemd160")
        ripemd.update(sha)
        return ripemd.digest()
    except ValueError:
        from Crypto.Hash import RIPEMD160
        h = RIPEMD160.new()
        h.update(sha)
        return h.digest()



def _encode_base58(data: bytes) -> str:
    num = int.from_bytes(data, "big")
    encode = ""
    while num > 0:
        num, rem = divmod(num, 58)
        encode = _BASE58_ALPHABET[rem] + encode
    pad = 0
    for byte in data:
        if byte == 0:
            pad += 1
        else:
            break
    return "1" * pad + encode


def _decode_base58(data: str) -> bytes:
    num = 0
    for char in data:
        num = num * 58 + _BASE58_ALPHABET.index(char)
    combined = num.to_bytes((num.bit_length() + 7) // 8 or 1, "big")
    pad = len(data) - len(data.lstrip("1"))
    return b"\x00" * pad + combined


def _base58_check_encode(payload: bytes) -> str:
    checksum = hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
    return _encode_base58(payload + checksum)


def _base58_check_decode(data: str) -> bytes:
    raw = _decode_base58(data)
    if len(raw) < 4:
        raise ValueError("Base58 payload too short")
    payload, checksum = raw[:-4], raw[-4:]
    expected = hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
    if checksum != expected:
        raise ValueError("Checksum mismatch")
    return payload


def _inverse_mod(value: int, modulus: int) -> int:
    return pow(value, -1, modulus)


def _is_on_curve(point: Optional[Tuple[int, int]]) -> bool:
    if point is None:
        return True
    x, y = point
    return (y * y - (x * x * x + 7)) % _P == 0


def _point_add(point1: Optional[Tuple[int, int]], point2: Optional[Tuple[int, int]]) -> Optional[Tuple[int, int]]:
    if point1 is None:
        return point2
    if point2 is None:
        return point1
    x1, y1 = point1
    x2, y2 = point2
    if x1 == x2 and y1 != y2:
        return None
    if x1 == x2:
        m = (3 * x1 * x1) * _inverse_mod(2 * y1 % _P, _P) % _P
    else:
        m = (y1 - y2) * _inverse_mod((x1 - x2) % _P, _P) % _P
    x3 = (m * m - x1 - x2) % _P
    y3 = (m * (x1 - x3) - y1) % _P
    return x3, y3


def _scalar_mult(k: int, point: Optional[Tuple[int, int]]) -> Optional[Tuple[int, int]]:
    if k % _N == 0 or point is None:
        return None
    result: Optional[Tuple[int, int]] = None
    addend = point
    while k:
        if k & 1:
            result = _point_add(result, addend)
        addend = _point_add(addend, addend)
        k >>= 1
    return result


def _lift_x(x: int) -> Tuple[int, int]:
    alpha = (x * x * x + 7) % _P
    beta = pow(alpha, (_P + 1) // 4, _P)
    if (_point := (x, beta)) and _is_on_curve(_point):
        return _point
    raise ValueError("Failed to lift x to curve")


def _parse_public_key(data: bytes) -> Tuple[int, int]:
    if len(data) == 33 and data[0] in (0x02, 0x03):
        x = int.from_bytes(data[1:], "big")
        x_point = _lift_x(x)
        if (x_point[1] % 2 == 0) != (data[0] == 0x02):
            x_point = (x_point[0], _P - x_point[1])
        return x_point
    if len(data) == 65 and data[0] == 0x04:
        x = int.from_bytes(data[1:33], "big")
        y = int.from_bytes(data[33:], "big")
        point = (x, y)
        if not _is_on_curve(point):
            raise ValueError("Invalid uncompressed public key")
        return point
    raise ValueError("Unsupported public key format")


def _deterministic_k(privkey: int, digest: bytes) -> int:
    priv_bytes = _int_to_bytes(privkey, 32)
    v = b"\x01" * 32
    k = b"\x00" * 32
    k = hmac.new(k, v + b"\x00" + priv_bytes + digest, hashlib.sha256).digest()
    v = hmac.new(k, v, hashlib.sha256).digest()
    k = hmac.new(k, v + b"\x01" + priv_bytes + digest, hashlib.sha256).digest()
    v = hmac.new(k, v, hashlib.sha256).digest()
    while True:
        v = hmac.new(k, v, hashlib.sha256).digest()
        candidate = int.from_bytes(v, "big")
        if 1 <= candidate < _N:
            return candidate
        k = hmac.new(k, v + b"\x00", hashlib.sha256).digest()
        v = hmac.new(k, v, hashlib.sha256).digest()


def _encode_der(r: int, s: int) -> bytes:
    def encode_int(value: int) -> bytes:
        raw = value.to_bytes((value.bit_length() + 7) // 8 or 1, "big")
        if raw[0] & 0x80:
            raw = b"\x00" + raw
        return raw

    r_bytes = encode_int(r)
    s_bytes = encode_int(s)
    sequence = b"\x02" + len(r_bytes).to_bytes(1, "big") + r_bytes
    sequence += b"\x02" + len(s_bytes).to_bytes(1, "big") + s_bytes
    return b"\x30" + len(sequence).to_bytes(1, "big") + sequence


def _decode_der(signature: bytes) -> Tuple[int, int]:
    if len(signature) < 8 or signature[0] != 0x30:
        raise ValueError("Invalid DER signature header")
    total_len = signature[1]
    if total_len + 2 != len(signature):
        raise ValueError("DER length mismatch")
    offset = 2
    if signature[offset] != 0x02:
        raise ValueError("DER missing R integer tag")
    r_len = signature[offset + 1]
    r_start = offset + 2
    r_end = r_start + r_len
    r = int.from_bytes(signature[r_start:r_end], "big")
    offset = r_end
    if signature[offset] != 0x02:
        raise ValueError("DER missing S integer tag")
    s_len = signature[offset + 1]
    s_start = offset + 2
    s_end = s_start + s_len
    s = int.from_bytes(signature[s_start:s_end], "big")
    if s_end != len(signature):
        raise ValueError("Unexpected DER trailer")
    if not (1 <= r < _N and 1 <= s < _N):
        raise ValueError("Signature scalars out of range")
    return r, s


def _p2pkh_script(pubkey_hash: bytes) -> bytes:
    if len(pubkey_hash) != 20:
        raise ValueError("Invalid pubkey hash length")
    return b"\x76\xa9\x14" + pubkey_hash + b"\x88\xac"


def _encode_push(data: bytes) -> bytes:
    if len(data) >= 0x4C:
        raise ValueError("Pushdata too large for simple encoding")
    return len(data).to_bytes(1, "big") + data


def address_to_script(address: str) -> bytes:
    """Convert a base58 P2PKH address into its locking script."""

    payload = _base58_check_decode(address)
    if payload[0] != PUBKEY_ADDRESS_PREFIX:
        raise ValueError("Address prefix mismatch")
    return _p2pkh_script(payload[1:])


@dataclass
class UTXO:
    txid: bytes
    index: int
    value: int
    address: str
    script_pubkey: bytes


@dataclass
class KeyPair:
    private_key: int
    compressed: bool = True

    @classmethod
    def generate(cls) -> "KeyPair":
        while True:
            secret = secrets.randbelow(_N)
            if 1 <= secret < _N:
                return cls(secret)

    @classmethod
    def from_wif(cls, wif: str) -> "KeyPair":
        payload = _base58_check_decode(wif)
        if payload[0] != 0x80:
            raise ValueError("Unsupported WIF prefix")
        compressed = False
        key_bytes = payload[1:]
        if len(key_bytes) == 33 and key_bytes[-1] == 0x01:
            compressed = True
            key_bytes = key_bytes[:-1]
        if len(key_bytes) != 32:
            raise ValueError("Invalid key length")
        secret = int.from_bytes(key_bytes, "big")
        if not 1 <= secret < _N:
            raise ValueError("Invalid secret exponent")
        return cls(secret, compressed=compressed)

    def to_wif(self) -> str:
        payload = b"\x80" + _int_to_bytes(self.private_key, 32)
        if self.compressed:
            payload += b"\x01"
        return _base58_check_encode(payload)

    def public_point(self) -> Tuple[int, int]:
        point = _scalar_mult(self.private_key, _G)
        if point is None or not _is_on_curve(point):
            raise ValueError("Derived invalid public point")
        return point

    def public_key(self) -> bytes:
        x, y = self.public_point()
        if self.compressed:
            prefix = b"\x02" if y % 2 == 0 else b"\x03"
            return prefix + _int_to_bytes(x, 32)
        return b"\x04" + _int_to_bytes(x, 32) + _int_to_bytes(y, 32)

    def address(self) -> str:
        pubkey_hash = _hash160(self.public_key())
        payload = bytes([PUBKEY_ADDRESS_PREFIX]) + pubkey_hash
        return _base58_check_encode(payload)

    def sign(self, digest: bytes) -> bytes:
        if len(digest) != 32:
            raise ValueError("Digest must be 32 bytes")
        while True:
            k = _deterministic_k(self.private_key, digest)
            point = _scalar_mult(k, _G)
            if point is None:
                continue
            r = point[0] % _N
            if r == 0:
                continue
            k_inv = _inverse_mod(k, _N)
            z = int.from_bytes(digest, "big")
            s = (k_inv * (z + r * self.private_key)) % _N
            if s == 0:
                continue
            if s > _N // 2:
                s = _N - s
            der = _encode_der(r, s)
            return der + b"\x01"  # SIGHASH_ALL

    def sign_raw(self, digest: bytes) -> bytes:
        """Produce a canonical DER signature without a sighash byte."""

        if len(digest) != 32:
            raise ValueError("Digest must be 32 bytes")
        while True:
            k = _deterministic_k(self.private_key, digest)
            point = _scalar_mult(k, _G)
            if point is None:
                continue
            r = point[0] % _N
            if r == 0:
                continue
            k_inv = _inverse_mod(k, _N)
            z = int.from_bytes(digest, "big")
            s = (k_inv * (z + r * self.private_key)) % _N
            if s == 0:
                continue
            if s > _N // 2:
                s = _N - s
            return _encode_der(r, s)


class Wallet:
    """Manage key pairs, unspent outputs, and signed transactions."""

    def __init__(self) -> None:
        self._keys: Dict[str, KeyPair] = {}
        self._utxos: Dict[Tuple[bytes, int], UTXO] = {}

    def import_wif(self, wif: str) -> str:
        key = KeyPair.from_wif(wif)
        address = key.address()
        self._keys[address] = key
        return address

    def get_wif(self, address: str) -> str:
        """Export WIF for a specific address."""
        if address not in self._keys:
            raise ValueError(f"Address {address} not found in wallet")
        return self._keys[address].to_wif()

    def new_address(self, compressed: bool = True) -> str:
        key = KeyPair.generate()
        key.compressed = compressed
        address = key.address()
        self._keys[address] = key
        return address

    def addresses(self) -> Iterable[str]:
        return self._keys.keys()

    def balance(self) -> int:
        return sum(utxo.value for utxo in self._utxos.values())

    def default_address(self) -> str:
        try:
            return next(iter(self._keys))
        except StopIteration as exc:
            raise RuntimeError("Wallet has no keys") from exc

    def register_utxo(self, txid: bytes, index: int, value: int, address: Optional[str] = None) -> None:
        owner = address or self.default_address()
        key = self._keys.get(owner)
        if key is None:
            raise ValueError("Unknown address for wallet")
        pubkey_hash = _hash160(key.public_key())
        script_pubkey = _p2pkh_script(pubkey_hash)
        utxo = UTXO(txid=txid, index=index, value=value, address=owner, script_pubkey=script_pubkey)
        self._utxos[(txid, index)] = utxo

    def _select_utxos(self, amount: int) -> Tuple[List[UTXO], int]:
        sorted_utxos = sorted(self._utxos.values(), key=lambda utxo: utxo.value, reverse=True)
        selected: List[UTXO] = []
        total = 0
        for utxo in sorted_utxos:
            selected.append(utxo)
            total += utxo.value
            if total >= amount:
                break
        if total < amount:
            raise ValueError("Insufficient funds")
        return selected, total

    def _address_to_script(self, address: str) -> bytes:
        return address_to_script(address)

    def create_transaction(self, payments: List[Tuple[str, int]], fee: int = 0) -> Transaction:
        if not payments:
            raise ValueError("At least one payment required")
        total_out = sum(amount for _, amount in payments) + fee
        utxos, gathered = self._select_utxos(total_out)
        change = gathered - total_out
        inputs = [
            TxInput(prev_txid=utxo.txid, prev_index=utxo.index, script_sig=b"")
            for utxo in utxos
        ]
        outputs = [
            TxOutput(value=amount, script_pubkey=self._address_to_script(address))
            for address, amount in payments
        ]
        if change > 0:
            change_address = utxos[0].address
            outputs.append(TxOutput(value=change, script_pubkey=self._address_to_script(change_address)))
        unsigned = Transaction(version=1, inputs=inputs, outputs=outputs)
        signed_inputs = []
        for idx, utxo in enumerate(utxos):
            key = self._keys[utxo.address]
            sighash = self._signature_hash(unsigned, idx, utxo.script_pubkey)
            signature = key.sign(sighash)
            script_sig = _encode_push(signature) + _encode_push(key.public_key())
            signed_inputs.append(
                TxInput(
                    prev_txid=utxo.txid,
                    prev_index=utxo.index,
                    script_sig=script_sig,
                    sequence=0xFFFFFFFF,
                )
            )
        final_tx = Transaction(version=unsigned.version, inputs=signed_inputs, outputs=unsigned.outputs, locktime=unsigned.locktime)
        for utxo in utxos:
            self._utxos.pop((utxo.txid, utxo.index), None)
        if change > 0:
            txid = final_tx.txid()
            change_index = len(outputs) - 1
            self.register_utxo(txid, change_index, change, utxos[0].address)
        return final_tx

    def _signature_hash(self, tx: Transaction, input_index: int, script_pubkey: bytes) -> bytes:
        tmp_inputs = []
        for idx, txin in enumerate(tx.inputs):
            script = script_pubkey if idx == input_index else b""
            tmp_inputs.append(
                TxInput(
                    prev_txid=txin.prev_txid,
                    prev_index=txin.prev_index,
                    script_sig=script,
                    sequence=txin.sequence,
                )
            )
        temp_tx = Transaction(version=tx.version, inputs=tmp_inputs, outputs=tx.outputs, locktime=tx.locktime)
        serialized = temp_tx.serialize() + b"\x01\x00\x00\x00"  # SIGHASH_ALL
        return sha256d(serialized)


def verify_der_signature(pubkey: bytes, digest: bytes, signature: bytes) -> bool:
    """Verify a DER-encoded secp256k1 signature against `digest`."""

    if len(digest) != 32:
        raise ValueError("Digest must be 32 bytes")
    try:
        point = _parse_public_key(pubkey)
        r, s = _decode_der(signature)
    except ValueError:
        return False
    z = int.from_bytes(digest, "big")
    s_inv = _inverse_mod(s, _N)
    u1 = (z * s_inv) % _N
    u2 = (r * s_inv) % _N
    check = _point_add(_scalar_mult(u1, _G), _scalar_mult(u2, point))
    if check is None:
        return False
    return check[0] % _N == r


__all__ = [
    "KeyPair",
    "Wallet",
    "verify_der_signature",
    "address_to_script",
]

