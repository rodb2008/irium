"""Minimal blockchain state machine for Irium."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Dict, List, Tuple

from .block import Block, BlockHeader
from .constants import BLOCK_TARGET_INTERVAL, DIFFICULTY_RETARGET_INTERVAL, SUBSIDY_SCHEDULE
from .pow import Target
from .tx import Transaction, TxOutput


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
    utxos: Dict[Tuple[bytes, int], TxOutput] = field(default_factory=dict)

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
        self._validate_and_apply_transactions(block, block_reward, enforce_reward=True)
        self.chain.append(block)
        self.height += 1
        self.total_work += int(0xFFFF_FFFF / block.header.target.to_target())

    def _connect_genesis(self, block: Block) -> None:
        if self.chain:
            raise ValueError("Genesis block already connected")
        self._validate_block_header(block, expected_height=0, previous=None)
        self._validate_and_apply_transactions(block, block_reward=0, enforce_reward=False)
        self.chain.append(block)
        self.height = 1
        self.total_work = int(0xFFFF_FFFF / block.header.target.to_target())

    def _validate_block_header(self, block: Block, height: int, previous: Block | None) -> None:
        if previous is not None:
            if block.header.prev_hash != previous.header.hash():
                raise ValueError("Block does not extend the current tip")
        elif block.header.prev_hash != b"\x00" * 32:
            raise ValueError("Genesis block must reference null hash")

        recalculated_root = block.merkle_root()[::-1]
        if block.header.merkle_root != recalculated_root:
            raise ValueError("Block merkle root mismatch")

        header_hash = block.header.hash()[::-1]
        target = self.target_for_height(height)
        if int.from_bytes(header_hash, "big") > target.to_target():
            raise ValueError("Block does not satisfy proof-of-work target")

    def _validate_and_apply_transactions(self, block: Block, block_reward: int, *, enforce_reward: bool) -> None:
        if not block.transactions:
            raise ValueError("Block must include transactions")
        coinbase = block.transactions[0]
        if not _is_coinbase(coinbase):
            raise ValueError("First transaction must be coinbase")
        if not coinbase.outputs:
            raise ValueError("Coinbase transaction must create outputs")

        created: List[Tuple[bytes, int, TxOutput]] = []
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
                utxo = self.utxos.get(key)
                if utxo is None:
                    raise ValueError("Referenced UTXO is missing")
                seen_inputs.add(key)
                input_total += utxo.value
            output_total = sum(output.value for output in tx.outputs)
            if input_total < output_total:
                raise ValueError("Transaction spends more than available inputs")
            fees += input_total - output_total
            txid = tx.txid()
            for index, output in enumerate(tx.outputs):
                created.append((txid, index, output))

        coinbase_total = sum(output.value for output in coinbase.outputs)
        if enforce_reward and coinbase_total > block_reward + fees:
            raise ValueError("Coinbase transaction exceeds allowed reward")

        coinbase_txid = coinbase.txid()
        for index, output in enumerate(coinbase.outputs):
            created.append((coinbase_txid, index, output))

        for key in seen_inputs:
            self.utxos.pop(key, None)
        for txid, index, output in created:
            self.utxos[(txid, index)] = output


def _is_coinbase(tx: Transaction) -> bool:
    if len(tx.inputs) != 1:
        return False
    coinbase_input = tx.inputs[0]
    return (
        coinbase_input.prev_txid == b"\x00" * 32
        and coinbase_input.prev_index == 0xFFFFFFFF
    )
