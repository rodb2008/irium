from pathlib import Path

from irium.tools.genesis_loader import load_locked_genesis


def test_locked_genesis_matches_header():
    block, payload = load_locked_genesis(Path.cwd())
    header = payload["header"]
    assert block.header.hash().hex() == header["hash"].lower()
    assert header["hash"].startswith("00000000")


def test_coinbase_transaction_present():
    _, payload = load_locked_genesis(Path.cwd())
    txs = payload.get("transactions", [])
    assert len(txs) == 1
    assert txs[0].startswith("0100000001")
