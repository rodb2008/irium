from pathlib import Path

from irium.chain import ChainParams, ChainState
from irium.pow import Target
from irium.tools.genesis_loader import load_locked_genesis


def test_chain_state_initialises_with_locked_genesis():
    block, payload = load_locked_genesis(Path.cwd())
    header = payload["header"]
    params = ChainParams(genesis_block=block, pow_limit=Target(bits=int(header["bits"], 16)))
    state = ChainState(params=params)
    assert state.height == 1
    assert state.chain[0].header.hash() == block.header.hash()
