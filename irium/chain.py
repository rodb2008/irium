"""Minimal blockchain state machine for Irium."""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import Dict, List, Tuple

from .block import Block, BlockHeader
from .constants import (
    BLOCK_TARGET_INTERVAL,
    COINBASE_MATURITY,
    DIFFICULTY_RETARGET_INTERVAL,
    MAX_FUTURE_BLOCK_TIME,
    MAX_MONEY,
    SUBSIDY_SCHEDULE,
)
from .pow import Target
from .tx import Transaction, TxOutput
from .wallet import verify_der_signature


@dataclass
class UTXOEntry:
    """UTXO with height tracking for coinbase maturity."""
    output: TxOutput
    height: int
    is_coinbase: bool


@dataclass
class ChainParams:
    genesis_block: Block
    pow_limit: Target


@dataclass
class ChainState:
    params: ChainParams
    height: int = 0
    chain: List[Block] = field(default_factory=list)
    total_work: int = 0
    utxos: Dict[Tuple[bytes, int], UTXOEntry] = field(default_factory=dict)
    issued: int = 0

    def __post_init__(self) -> None:
        self._connect_genesis(self.params.genesis_block)

    def expected_time(self, height: int) -> int:
        return height * BLOCK_TARGET_INTERVAL

    def target_for_height(self, height: int) -> Target:
        if height == 0:
            return self.params.genesis_block.header.target
        last_block = self.chain[-1]
        if height % DIFFICULTY_RETARGET_INTERVAL != 0 or height < DIFFICULTY_RETARGET_INTERVAL:
            return last_block.header.target
        prev = self.chain[-DIFFICULTY_RETARGET_INTERVAL]
        actual_time = last_block.header.time - prev.header.time
        expected_time = DIFFICULTY_RETARGET_INTERVAL * BLOCK_TARGET_INTERVAL
        adjustment = max(min(actual_time / expected_time, 4), 0.25)
        new_target_value = int(last_block.header.target.to_target() * adjustment)
        return Target.from_target(new_target_value)

    def connect_block(self, block: Block) -> None:
        expected_height = self.height
        previous = self.chain[-1] if self.chain else None
        self._validate_block_header(block, expected_height, previous)
        block_reward = SUBSIDY_SCHEDULE.block_reward(expected_height)
        fees, coinbase_total, subsidy_created = self._validate_and_apply_transactions(
            block,
            block_reward,
            expected_height,
            enforce_reward=True,
            max_subsidy=MAX_MONEY - self.issued,
        )
        new_supply = self.issued + subsidy_created
        self.chain.append(block)
        self.height += 1
        self.total_work += int(0xFFFF_FFFF / block.header.target.to_target())
        self.issued = new_supply

    def _connect_genesis(self, block: Block) -> None:
        if self.chain:
            raise ValueError("Genesis block already connected")
        self._validate_block_header(block, height=0, previous=None)
        fees, coinbase_total, subsidy_created = self._validate_and_apply_transactions(
            block,
            block_reward=0,
            height=0,
            enforce_reward=False,
            max_subsidy=MAX_MONEY,
        )
        self.chain.append(block)
        self.height = 1
        self.total_work = int(0xFFFF_FFFF / block.header.target.to_target())
        self.issued = subsidy_created

    def _validate_block_header(self, block: Block, height: int, previous: Block | None) -> None:
        if previous is not None and not (height == 0 and block.header.prev_hash in (b"\x00" * 32, bytes.fromhex("0000000040e3eb5ed9db5cc8df56dd6db9c6f3009ca7e9114fb52400e0136fb6"))):
            if block.header.prev_hash != previous.header.hash():
                raise ValueError("Block does not extend the current tip")
        elif block.header.prev_hash != b"\x00" * 32:
            raise ValueError("Genesis block must reference null hash")

        # FIX 2: TIMESTAMP VALIDATION (Whitepaper requirement)
        current_time = int(time.time())
        if block.header.time > current_time + MAX_FUTURE_BLOCK_TIME:
            raise ValueError(f"Block timestamp too far in future (max {MAX_FUTURE_BLOCK_TIME}s)")
        
        if previous is not None and block.header.time <= previous.header.time:
            raise ValueError("Block timestamp must be greater than previous block")

        recalculated_root = block.merkle_root()[::-1]
        if block.header.merkle_root != recalculated_root:
            raise ValueError("Block merkle root mismatch")

        header_hash = block.header.hash()
        target = self.target_for_height(height)
        
        # SECURITY FIX: Validate that block.header.bits matches expected bits
        if block.header.target.bits != target.bits:
            raise ValueError(f"Block bits mismatch: got {hex(block.header.target.bits)}, expected {hex(target.bits)}")
        
        if int.from_bytes(header_hash, "big") > target.to_target():
            raise ValueError("Block does not satisfy proof-of-work target")

    def _validate_and_apply_transactions(
        self,
        block: Block,
        block_reward: int,
        height: int,
        *,
        enforce_reward: bool,
        max_subsidy: int | None = None,
    ) -> Tuple[int, int, int]:
        if not block.transactions:
            raise ValueError("Block must include transactions")
        coinbase = block.transactions[0]
        if not _is_coinbase(coinbase):
            raise ValueError("First transaction must be coinbase")
        if not coinbase.outputs:
            raise ValueError("Coinbase transaction must create outputs")

        created: List[Tuple[bytes, int, TxOutput, bool]] = []
        fees = 0
        seen_inputs: set[Tuple[bytes, int]] = set()

        for tx in block.transactions[1:]:
            if not tx.inputs:
                raise ValueError("Transaction must have at least one input")
            if not tx.outputs:
                raise ValueError("Transaction must have at least one output")
            input_total = 0
            for txin in tx.inputs:
                key = (txin.prev_txid, txin.prev_index)
                if len(txin.prev_txid) != 32:
                    raise ValueError("Transaction input has invalid txid length")
                if not (0 <= txin.prev_index <= 0xFFFFFFFF):
                    raise ValueError("Transaction input index out of range")
                if key in seen_inputs:
                    raise ValueError("Transaction input double spent within block")
                utxo_entry = self.utxos.get(key)
                if utxo_entry is None:
                    raise ValueError("Referenced UTXO is missing")
                
                # FIX 1: COINBASE MATURITY CHECK (Whitepaper: 100 blocks)
                if utxo_entry.is_coinbase:
                    confirmations = height - utxo_entry.height
                    if confirmations < COINBASE_MATURITY:
                        raise ValueError(
                            f"Coinbase UTXO not mature (needs {COINBASE_MATURITY} confirmations, has {confirmations})"
                        )
                
                # FIX 3: SIGNATURE VERIFICATION (Whitepaper requirement)
                utxo = utxo_entry.output
                if not _verify_transaction_signature(txin, utxo):
                    raise ValueError("Transaction signature verification failed")
                
                seen_inputs.add(key)
                input_total += utxo.value
            output_total = 0
            for output in tx.outputs:
                _validate_output(output)
                output_total += output.value
            if input_total < output_total:
                raise ValueError("Transaction spends more than available inputs")
            fees += input_total - output_total
            if fees < 0 or fees > MAX_MONEY:
                raise ValueError("Fee accounting overflow")
            txid = tx.txid()
            for index, output in enumerate(tx.outputs):
                created.append((txid, index, output, False))

        coinbase_total = 0
        for output in coinbase.outputs:
            _validate_output(output)
            coinbase_total += output.value
            if coinbase_total > MAX_MONEY:
                raise ValueError("Coinbase outputs overflow")
        if enforce_reward and coinbase_total > block_reward + fees:
            raise ValueError("Coinbase transaction exceeds allowed reward")

        coinbase_txid = coinbase.txid()
        for index, output in enumerate(coinbase.outputs):
            created.append((coinbase_txid, index, output, True))

        available_fees = min(fees, coinbase_total)
        subsidy_created = coinbase_total - available_fees
        if subsidy_created < 0:
            subsidy_created = 0
        if enforce_reward:
            subsidy_created = min(block_reward, subsidy_created)
        if max_subsidy is not None and subsidy_created > max_subsidy:
            raise ValueError("Coinbase subsidy would exceed permitted supply")

        for key in seen_inputs:
            self.utxos.pop(key, None)
        for txid, index, output, is_coinbase in created:
            self.utxos[(txid, index)] = UTXOEntry(output, height, is_coinbase)

        return fees, coinbase_total, subsidy_created


def _is_coinbase(tx: Transaction) -> bool:
    if len(tx.inputs) != 1:
        return False
    coinbase_input = tx.inputs[0]
    return (
        coinbase_input.prev_txid == b"\x00" * 32
        and coinbase_input.prev_index == 0xFFFFFFFF
    )


def _validate_output(output: TxOutput) -> None:
    if not (0 <= output.value <= MAX_MONEY):
        raise ValueError("Output value out of range")
    if len(output.script_pubkey) > 0xFF:
        raise ValueError("script_pubkey too large")


def _verify_transaction_signature(txin, utxo: TxOutput) -> bool:
    """Verify transaction input signature against UTXO script_pubkey."""
    if len(utxo.script_pubkey) < 34:
        return False
    
    pubkey_len = utxo.script_pubkey[0]
    if pubkey_len not in (33, 65):
        return False
    
    if len(utxo.script_pubkey) < 1 + pubkey_len:
        return False
    
    pubkey = utxo.script_pubkey[1:1 + pubkey_len]
    
    if len(txin.script_sig) < 2:
        return False
    
    sig_len = txin.script_sig[0]
    if len(txin.script_sig) < 1 + sig_len:
        return False
    
    signature = txin.script_sig[1:1 + sig_len]
    digest = txin.prev_txid
    
    try:
        return verify_der_signature(pubkey, digest, signature)
    except Exception:
        return False
