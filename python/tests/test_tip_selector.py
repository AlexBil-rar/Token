# tests/test_tip_selector.py

from app.consensus.tip_selector import TipSelector
from app.ledger.dag import DAG
from app.ledger.transaction import TransactionVertex


def make_tx(tx_id: str, parents: list[str] = None, sender: str = "alice", weight: int = 1) -> TransactionVertex:
    tx = TransactionVertex(
        sender=sender,
        receiver="bob",
        amount=10,
        nonce=1,
        timestamp=1000,
        public_key="pk",
        parents=parents or [],
    )
    tx.tx_id = tx_id
    tx.weight = weight
    return tx

def test_empty_dag_returns_no_parents():
    dag = DAG()
    selector = TipSelector()
    assert selector.select(dag) == []


def test_single_tip_returns_it():
    dag = DAG()
    dag.add_transaction(make_tx("tx1"))
    selector = TipSelector()
    result = selector.select(dag)
    assert result == ["tx1"]


def test_returns_at_most_max_parents():
    dag = DAG()
    for i in range(5):
        dag.add_transaction(make_tx(f"tx{i}", sender=f"user{i}"))
    selector = TipSelector()
    result = selector.select(dag)
    assert len(result) <= 2 

def test_no_duplicates_in_result():
    dag = DAG()
    for i in range(3):
        dag.add_transaction(make_tx(f"tx{i}", sender=f"user{i}"))
    selector = TipSelector()
    for _ in range(50):
        result = selector.select(dag)
        assert len(result) == len(set(result))


def test_all_selected_are_valid_tips():
    dag = DAG()
    dag.add_transaction(make_tx("tx1"))
    dag.add_transaction(make_tx("tx2", sender="bob"))
    dag.add_transaction(make_tx("tx3", sender="carol"))
    selector = TipSelector()
    tips = set(dag.get_tips())
    for _ in range(20):
        result = selector.select(dag)
        for tip_id in result:
            assert tip_id in tips


def test_heavier_tip_selected_more_often():
    dag = DAG()
    dag.add_transaction(make_tx("tx_light", sender="alice", weight=1))
    dag.add_transaction(make_tx("tx_heavy", sender="bob", weight=100))

    selector = TipSelector()
    counts = {"tx_light": 0, "tx_heavy": 0}

    for _ in range(200):
        result = selector.select(dag, max_parents=1)
        counts[result[0]] += 1

    assert counts["tx_heavy"] > counts["tx_light"]