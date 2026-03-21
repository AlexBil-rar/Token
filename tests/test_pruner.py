# tests/test_pruner.py

from app.ledger.dag import DAG
from app.ledger.pruner import Pruner
from app.ledger.state import LedgerState
from app.ledger.transaction import (
    TX_STATUS_CONFIRMED,
    TX_STATUS_PENDING,
    TransactionVertex,
)


def make_confirmed_tx(tx_id: str, timestamp: int, parents: list[str] = None) -> TransactionVertex:
    tx = TransactionVertex(
        sender="alice",
        receiver="bob",
        amount=10,
        nonce=1,
        timestamp=timestamp,
        public_key="pk",
        parents=parents or [],
    )
    tx.tx_id = tx_id
    tx.status = TX_STATUS_CONFIRMED
    return tx


def make_pending_tx(tx_id: str, timestamp: int) -> TransactionVertex:
    tx = TransactionVertex(
        sender="alice",
        receiver="bob",
        amount=10,
        nonce=1,
        timestamp=timestamp,
        public_key="pk",
        parents=[],
    )
    tx.tx_id = tx_id
    tx.status = TX_STATUS_PENDING
    return tx


def test_should_prune_at_interval():
    pruner = Pruner(window=100)
    dag = DAG()
    for i in range(1000):
        tx = make_confirmed_tx(f"tx{i}", timestamp=i)
        dag.vertices[f"tx{i}"] = tx
    assert pruner.should_prune(dag, interval=1000)


def test_should_not_prune_below_interval():
    pruner = Pruner(window=100)
    dag = DAG()
    for i in range(500):
        tx = make_confirmed_tx(f"tx{i}", timestamp=i)
        dag.vertices[f"tx{i}"] = tx
    assert not pruner.should_prune(dag, interval=1000)


def test_prune_removes_old_confirmed():
    pruner = Pruner(window=5)
    dag = DAG()
    state = LedgerState()

    for i in range(10):
        tx = make_confirmed_tx(f"tx{i}", timestamp=i)
        dag.vertices[f"tx{i}"] = tx
        dag.tips.add(f"tx{i}")

    dag.tips = {"tx9"}

    result = pruner.prune(dag, state)

    assert result.pruned_count > 0
    assert result.remaining_count <= 10
    assert result.state_preserved is True


def test_prune_never_removes_tips():
    pruner = Pruner(window=2)
    dag = DAG()
    state = LedgerState()

    for i in range(10):
        tx = make_confirmed_tx(f"tx{i}", timestamp=i)
        dag.vertices[f"tx{i}"] = tx

    dag.tips = {f"tx{i}" for i in range(10)}

    result = pruner.prune(dag, state)

    assert result.pruned_count == 0
    assert len(dag.vertices) == 10


def test_prune_never_removes_pending():
    pruner = Pruner(window=2)
    dag = DAG()
    state = LedgerState()

    for i in range(5):
        tx = make_pending_tx(f"tx{i}", timestamp=i)
        dag.vertices[f"tx{i}"] = tx

    result = pruner.prune(dag, state)

    assert result.pruned_count == 0


def test_prune_preserves_state():
    pruner = Pruner(window=2)
    dag = DAG()
    state = LedgerState()
    state.credit("alice", 1000)
    state.credit("bob", 500)

    for i in range(10):
        tx = make_confirmed_tx(f"tx{i}", timestamp=i)
        dag.vertices[f"tx{i}"] = tx
    dag.tips = {"tx9"}

    pruner.prune(dag, state)

    assert state.balances["alice"] == 1000
    assert state.balances["bob"] == 500


def test_prune_within_window_does_nothing():
    pruner = Pruner(window=1000)
    dag = DAG()
    state = LedgerState()

    for i in range(10):
        tx = make_confirmed_tx(f"tx{i}", timestamp=i)
        dag.vertices[f"tx{i}"] = tx

    result = pruner.prune(dag, state)

    assert result.pruned_count == 0
    assert result.remaining_count == 10


def test_prune_removes_oldest_first():
    pruner = Pruner(window=3)
    dag = DAG()
    state = LedgerState()

    for i in range(6):
        tx = make_confirmed_tx(f"tx{i}", timestamp=i)
        dag.vertices[f"tx{i}"] = tx

    dag.tips = {"tx5"} 

    pruner.prune(dag, state)

    assert "tx0" not in dag.vertices
    assert "tx1" not in dag.vertices
    assert "tx2" not in dag.vertices
    assert "tx5" in dag.vertices


def test_pruner_stats():
    pruner = Pruner(window=5)
    dag = DAG()

    for i in range(8):
        tx = make_confirmed_tx(f"tx{i}", timestamp=i)
        dag.vertices[f"tx{i}"] = tx

    stats = pruner.stats(dag)
    assert stats["total_vertices"] == 8
    assert stats["confirmed"] == 8
    assert stats["window"] == 5
    assert stats["prunable"] == 3