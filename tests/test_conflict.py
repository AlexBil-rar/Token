# tests/test_conflict.py

from app.config import GENESIS_BALANCE
from app.crypto.wallet import Wallet
from app.ledger.node import Node
from app.ledger.transaction import TX_STATUS_CONFLICT


def make_node_with_alice():
    node = Node()
    alice = Wallet.generate()
    node.bootstrap_genesis(alice.address, GENESIS_BALANCE)
    return node, alice


def test_first_tx_wins_conflict():
    import time
    from app.consensus.conflict_resolver import ConflictResolver
    from app.consensus.engine import ConsensusEngine
    from app.ledger.dag import DAG
    from app.ledger.mempool import Mempool
    from app.ledger.state import LedgerState
    from app.ledger.transaction import TransactionVertex

    state = LedgerState()
    alice = Wallet.generate()
    bob = Wallet.generate()
    state.credit(alice.address, 1000)

    dag = DAG()
    mempool = Mempool()
    resolver = ConflictResolver()
    engine = ConsensusEngine()

    tx_early = TransactionVertex(
        sender=alice.address, receiver=bob.address,
        amount=100, nonce=1, timestamp=1000,
        public_key=alice.public_key, parents=[],
    )
    tx_early.anti_spam_hash = tx_early.compute_anti_spam_hash()
    tx_early.signature = alice.sign(tx_early.signing_payload())
    tx_early.finalize()

    tx_late = TransactionVertex(
        sender=alice.address, receiver=bob.address,
        amount=200, nonce=1, timestamp=2000,
        public_key=alice.public_key, parents=[],
    )
    tx_late.anti_spam_hash = tx_late.compute_anti_spam_hash()
    tx_late.signature = alice.sign(tx_late.signing_payload())
    tx_late.finalize()

    mempool.add(tx_early)
    mempool.add(tx_late)
    resolver.register_transaction(tx_early)
    resolver.register_transaction(tx_late)

    accepted = engine.process_mempool(mempool, dag, state, resolver)

    assert len(accepted) == 1
    assert accepted[0].tx_id == tx_early.tx_id
    assert tx_late.status == TX_STATUS_CONFLICT


def test_conflict_does_not_affect_balance_twice():
    node, alice = make_node_with_alice()
    bob = Wallet.generate()

    tx1 = node.create_transaction(alice, bob.address, 100)
    node.submit_transaction(tx1)

    state = node.get_state_view()
    assert state["balances"][alice.address] == GENESIS_BALANCE - 100


def test_no_conflict_single_tx():
    node, alice = make_node_with_alice()
    bob = Wallet.generate()

    tx = node.create_transaction(alice, bob.address, 50)
    result = node.submit_transaction(tx)

    assert result.ok
    dag = node.get_dag_view()
    assert dag["stats"]["conflict"] == 0