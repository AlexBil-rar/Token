# tests/test_validator.py
 
import pytest
from app.config import ANTI_SPAM_DIFFICULTY
from app.crypto.wallet import Wallet
from app.ledger.dag import DAG
from app.ledger.state import LedgerState
from app.ledger.transaction import TransactionVertex, TX_STATUS_REJECTED
from app.ledger.validator import Validator
 
 
def make_valid_tx(wallet: Wallet, dag: DAG, state: LedgerState, amount: int = 10) -> TransactionVertex:
    import time
    from app.crypto.hashing import sha256_hex
 
    nonce = state.get_nonce(wallet.address) + 1
    parents = list(dag.tips)[:2] if dag.vertices else []
 
    tx = TransactionVertex(
        sender=wallet.address,
        receiver="receiver123",
        amount=amount,
        nonce=nonce,
        timestamp=int(time.time()),
        public_key=wallet.public_key,
        parents=parents,
    )
 
    nonce_val = 0
    while True:
        tx.anti_spam_nonce = nonce_val
        tx.anti_spam_hash = tx.compute_anti_spam_hash()
        if tx.anti_spam_hash.startswith("0" * ANTI_SPAM_DIFFICULTY):
            break
        nonce_val += 1
 
    tx.signature = wallet.sign(tx.signing_payload())
    tx.finalize()
    return tx
 
 
def test_structure_valid():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    state.credit(wallet.address, 100)
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
    result = v.validate_structure(tx)
    assert result.ok
 
 
def test_structure_empty_sender():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
    tx.sender = ""
    result = v.validate_structure(tx)
    assert not result.ok
    assert result.code == "bad_sender"
 
 
def test_structure_zero_amount():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
    tx.amount = 0
    result = v.validate_structure(tx)
    assert not result.ok
    assert result.code == "bad_amount"
 
 
def test_structure_too_many_parents():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
    tx.parents = ["a", "b", "c"] 
    result = v.validate_structure(tx)
    assert not result.ok
    assert result.code == "bad_parents"
 
 
def test_anti_spam_valid():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    state.credit(wallet.address, 100)
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
    result = v.validate_anti_spam(tx)
    assert result.ok
 
 
def test_anti_spam_wrong_hash():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
    tx.anti_spam_hash = "badhash"
    result = v.validate_anti_spam(tx)
    assert not result.ok
    assert result.code == "bad_pow"
 
 
def test_state_insufficient_balance():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    state.credit(wallet.address, 5)
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state, amount=10)
    result = v.validate_state(tx, state)
    assert not result.ok
    assert result.code == "bad_state"
 
 
def test_state_sufficient_balance():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    state.credit(wallet.address, 100)
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state, amount=10)
    result = v.validate_state(tx, state)
    assert result.ok
 
 
def test_duplicate_detected():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    state.credit(wallet.address, 100)
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
    dag.add_transaction(tx)
 
    result = v.validate_duplicate(tx, dag)
    assert not result.ok
    assert result.code == "duplicate"
 
 
def test_no_duplicate():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    state.credit(wallet.address, 100)
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
 
    result = v.validate_duplicate(tx, dag)
    assert result.ok
 
 
def test_full_validation_passes():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    state.credit(wallet.address, 100)
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
    result = v.validate_full(tx, dag, state)
    assert result.ok
 
 
def test_full_validation_fails_on_bad_amount():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
    tx.amount = -1
    result = v.validate_full(tx, dag, state)
    assert not result.ok


def test_signature_valid():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    state.credit(wallet.address, 100)
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
    result = v.validate_signature(tx)
    assert result.ok
 

def test_signature_wrong_key():
    v = Validator()
    wallet_a = Wallet.generate()
    wallet_b = Wallet.generate()
    state = LedgerState()
    state.credit(wallet_a.address, 100)
    dag = DAG()
    tx = make_valid_tx(wallet_a, dag, state)
 
    tx.public_key = wallet_b.public_key
    result = v.validate_signature(tx)
    assert not result.ok
 
 
def test_signature_tampered_payload():
    v = Validator()
    wallet = Wallet.generate()
    state = LedgerState()
    state.credit(wallet.address, 100)
    dag = DAG()
    tx = make_valid_tx(wallet, dag, state)
 
    tx.amount = 9999
    result = v.validate_signature(tx)
    assert not result.ok
