# tests/test_snapshot.py

import pytest
from pathlib import Path
from app.config import GENESIS_BALANCE
from app.crypto.wallet import Wallet
from app.ledger.dag import DAG
from app.ledger.node import Node
from app.ledger.state import LedgerState
from app.storage.snapshot import SnapshotStorage


@pytest.fixture
def tmp_snapshot(tmp_path):
    return SnapshotStorage(path=tmp_path / "test_snapshot.json")


def test_save_and_load_empty(tmp_snapshot):
    dag = DAG()
    state = LedgerState()
    tmp_snapshot.save(dag, state)
    assert tmp_snapshot.exists()

    dag2 = DAG()
    state2 = LedgerState()
    loaded = tmp_snapshot.load(dag2, state2)
    assert loaded
    assert len(dag2.vertices) == 0


def test_save_and_load_with_balance(tmp_snapshot):
    dag = DAG()
    state = LedgerState()
    state.credit("alice", 1000)

    tmp_snapshot.save(dag, state)

    dag2 = DAG()
    state2 = LedgerState()
    tmp_snapshot.load(dag2, state2)

    assert state2.balances["alice"] == 1000


def test_save_and_load_transactions(tmp_snapshot):
    node = Node(storage=tmp_snapshot)
    alice = Wallet.generate()
    bob = Wallet.generate()
    node.bootstrap_genesis(alice.address, GENESIS_BALANCE)

    tx = node.create_transaction(alice, bob.address, 100)
    node.submit_transaction(tx)

    node2 = Node(storage=tmp_snapshot)
    loaded = node2.load_snapshot()

    assert loaded
    assert node2.dag.has_transaction(tx.tx_id)
    assert node2.state.balances[bob.address] == 100
    assert node2.state.balances[alice.address] == GENESIS_BALANCE - 100


def test_node_resumes_nonce(tmp_snapshot):
    node = Node(storage=tmp_snapshot)
    alice = Wallet.generate()
    bob = Wallet.generate()
    node.bootstrap_genesis(alice.address, GENESIS_BALANCE)

    tx1 = node.create_transaction(alice, bob.address, 10)
    node.submit_transaction(tx1)
    tx2 = node.create_transaction(alice, bob.address, 10)
    node.submit_transaction(tx2)

    node2 = Node(storage=tmp_snapshot)
    node2.load_snapshot()

    assert node2.state.get_nonce(alice.address) == 2


def test_no_snapshot_returns_false(tmp_snapshot):
    dag = DAG()
    state = LedgerState()
    loaded = tmp_snapshot.load(dag, state)
    assert not loaded


def test_snapshot_updates_on_each_tx(tmp_snapshot):
    node = Node(storage=tmp_snapshot)
    alice = Wallet.generate()
    bob = Wallet.generate()
    node.bootstrap_genesis(alice.address, GENESIS_BALANCE)

    for amount in [10, 20, 30]:
        tx = node.create_transaction(alice, bob.address, amount)
        node.submit_transaction(tx)

    node2 = Node(storage=tmp_snapshot)
    node2.load_snapshot()

    assert len(node2.dag.vertices) == 3
    assert node2.state.balances[bob.address] == 60