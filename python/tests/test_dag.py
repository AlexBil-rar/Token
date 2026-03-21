    # tests/test_dag.py
 
import pytest
from app.config import CONFIRMATION_THRESHOLD
from app.ledger.dag import DAG
from app.ledger.transaction import (
    TX_STATUS_CONFIRMED,
    TX_STATUS_CONFLICT,
    TX_STATUS_PENDING,
    TransactionVertex,
)
 
 
def make_tx(tx_id: str, parents: list[str] = None, sender: str = "alice") -> TransactionVertex:
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
    return tx
 
 
def test_add_transaction_stores_vertex():
    dag = DAG()
    tx = make_tx("tx1")
    dag.add_transaction(tx)
    assert dag.has_transaction("tx1")
 
 
def test_add_transaction_adds_to_tips():
    dag = DAG()
    tx = make_tx("tx1")
    dag.add_transaction(tx)
    assert "tx1" in dag.get_tips()
 
 
def test_add_transaction_removes_parent_from_tips():
    dag = DAG()
    tx1 = make_tx("tx1")
    dag.add_transaction(tx1)
 
    tx2 = make_tx("tx2", parents=["tx1"])
    dag.add_transaction(tx2)
 
    assert "tx1" not in dag.get_tips()
    assert "tx2" in dag.get_tips()
 
 
def test_add_duplicate_raises():
    dag = DAG()
    tx = make_tx("tx1")
    dag.add_transaction(tx)
    with pytest.raises(ValueError):
        dag.add_transaction(tx)
 
 
def test_propagate_weight_increments_parent():
    dag = DAG()
    tx1 = make_tx("tx1")
    dag.add_transaction(tx1)
 
    tx2 = make_tx("tx2", parents=["tx1"])
    dag.add_transaction(tx2)
    dag.propagate_weight("tx2")
 
    assert dag.vertices["tx1"].weight == 2  
 
def test_propagate_weight_confirms_after_threshold():
    dag = DAG()
    tx0 = make_tx("tx0")
    dag.add_transaction(tx0)

    for i in range(1, 6):
        tx = make_tx(f"tx{i}", parents=["tx0"], sender=f"user{i}")
        dag.add_transaction(tx)
        dag.propagate_weight(f"tx{i}")

    assert dag.vertices["tx0"].status == TX_STATUS_CONFIRMED

 
 
def test_propagate_weight_does_not_confirm_below_threshold():
    dag = DAG()
    tx0 = make_tx("tx0")
    dag.add_transaction(tx0)

    for i in range(1, 5):
        tx = make_tx(f"tx{i}", parents=["tx0"], sender=f"user{i}")
        dag.add_transaction(tx)
        dag.propagate_weight(f"tx{i}")

    assert dag.vertices["tx0"].status == TX_STATUS_PENDING
 
 
def test_get_tips_excludes_conflict():
    dag = DAG()
    tx = make_tx("tx1")
    dag.add_transaction(tx)
    tx.status = TX_STATUS_CONFLICT
 
    assert "tx1" not in dag.get_tips()
 
 
def test_get_tips_excludes_rejected():
    from app.ledger.transaction import TX_STATUS_REJECTED
    dag = DAG()
    tx = make_tx("tx1")
    dag.add_transaction(tx)
    tx.status = TX_STATUS_REJECTED
 
    assert "tx1" not in dag.get_tips()
 
 
def test_stats_counts_correctly():
    from app.ledger.transaction import TX_STATUS_REJECTED
    dag = DAG()
 
    tx1 = make_tx("tx1")
    tx2 = make_tx("tx2", sender="bob")
    tx3 = make_tx("tx3", sender="carol")
 
    dag.add_transaction(tx1)
    dag.add_transaction(tx2)
    dag.add_transaction(tx3)
 
    tx2.status = TX_STATUS_CONFLICT
    tx3.status = TX_STATUS_REJECTED
 
    s = dag.stats()
    assert s["total_vertices"] == 3
    assert s["pending"] == 1
    assert s["conflict"] == 1
    assert s["rejected"] == 1
    assert s["confirmed"] == 0
