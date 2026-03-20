# tests/test_node.py
 
from app.config import GENESIS_BALANCE
from app.crypto.wallet import Wallet
from app.ledger.node import Node
from app.ledger.transaction import TX_STATUS_CONFIRMED, TX_STATUS_CONFLICT
 
 
def make_node_with_alice() -> tuple:
    node = Node()
    alice = Wallet.generate()
    node.bootstrap_genesis(alice.address, GENESIS_BALANCE)
    return node, alice
 
 
def test_successful_transaction():
    node, alice = make_node_with_alice()
    bob = Wallet.generate()
 
    tx = node.create_transaction(alice, bob.address, 100)
    result = node.submit_transaction(tx)
 
    assert result.ok
    assert result.code == "accepted"
 
 
def test_balance_updates_after_tx():
    node, alice = make_node_with_alice()
    bob = Wallet.generate()
 
    tx = node.create_transaction(alice, bob.address, 100)
    node.submit_transaction(tx)
 
    state = node.get_state_view()
    assert state["balances"][bob.address] == 100
    assert state["balances"][alice.address] == GENESIS_BALANCE - 100
 
 
def test_multiple_sequential_transactions():
    node, alice = make_node_with_alice()
    bob = Wallet.generate()
 
    for amount in [100, 50, 30]:
        tx = node.create_transaction(alice, bob.address, amount)
        result = node.submit_transaction(tx)
        assert result.ok
 
    state = node.get_state_view()
    assert state["balances"][bob.address] == 180
    assert state["balances"][alice.address] == GENESIS_BALANCE - 180
 
 
def test_insufficient_balance_rejected():
    node, alice = make_node_with_alice()
    bob = Wallet.generate()
 
    tx = node.create_transaction(alice, bob.address, GENESIS_BALANCE + 1)
    result = node.submit_transaction(tx)
 
    assert not result.ok
 
 
def test_duplicate_transaction_rejected():
    node, alice = make_node_with_alice()
    bob = Wallet.generate()
 
    tx = node.create_transaction(alice, bob.address, 100)
    node.submit_transaction(tx)
 
    result = node.submit_transaction(tx)
    assert not result.ok
 
 
def test_transaction_appears_in_dag():
    node, alice = make_node_with_alice()
    bob = Wallet.generate()
 
    tx = node.create_transaction(alice, bob.address, 100)
    node.submit_transaction(tx)
 
    dag_view = node.get_dag_view()
    assert tx.tx_id in dag_view["transactions"]
 
 
def test_dag_stats_after_transactions():
    node, alice = make_node_with_alice()
    bob = Wallet.generate()
 
    for amount in [100, 50, 30]:
        tx = node.create_transaction(alice, bob.address, amount)
        node.submit_transaction(tx)
 
    stats = node.get_dag_view()["stats"]
    assert stats["total_vertices"] == 3
    assert stats["rejected"] == 0
    assert stats["conflict"] == 0
 
 
def test_nonce_increments_per_sender():
    node, alice = make_node_with_alice()
    bob = Wallet.generate()
 
    tx1 = node.create_transaction(alice, bob.address, 10)
    node.submit_transaction(tx1)
 
    tx2 = node.create_transaction(alice, bob.address, 10)
    node.submit_transaction(tx2)
 
    state = node.get_state_view()
    assert state["nonces"][alice.address] == 2
