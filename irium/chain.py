"""Minimal blockchain state machine for Irium."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Dict, List, Tuple

from .block import Block, BlockHeader
from .constants import (
    BLOCK_TARGET_INTERVAL,
    DIFFICULTY_RETARGET_INTERVAL,
    MAX_MONEY,
    SUBSIDY_SCHEDULE,
)
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
        self._validate_block_header(block, expected_height=0, previous=None)
        fees, coinbase_total, subsidy_created = self._validate_and_apply_transactions(
            block,
            block_reward=0,
            enforce_reward=False,
            max_subsidy=MAX_MONEY,
        )
        self.chain.append(block)
        self.height = 1
        self.total_work = int(0xFFFF_FFFF / block.header.target.to_target())
        self.issued = subsidy_created

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

    def _validate_and_apply_transactions(
        self,
        block: Block,
        block_reward: int,
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
                created.append((txid, index, output))

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
            created.append((coinbase_txid, index, output))

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
        for txid, index, output in created:
            self.utxos[(txid, index)] = output

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
