# tests/test_stealth.py

from app.crypto.stealth import (
    StealthKeys,
    generate_stealth_payment,
    scan_for_payment,
)
from app.config import GENESIS_BALANCE
from app.crypto.wallet import Wallet
from app.ledger.node import Node


def test_generate_stealth_keys():
    keys = StealthKeys.generate()
    assert len(keys.spend_public) == 32
    assert len(keys.spend_private) == 32
    assert keys.spend_public_hex() != keys.spend_private_hex()


def test_stealth_keys_hex_roundtrip():
    keys = StealthKeys.generate()
    pub_hex = keys.spend_public_hex()
    priv_hex = keys.spend_private_hex()
    assert len(pub_hex) == 64
    assert len(priv_hex) == 64


def test_generate_stealth_payment_produces_address():
    bob_keys = StealthKeys.generate()
    payment = generate_stealth_payment(bob_keys.spend_public_hex())
    assert len(payment.stealth_address) == 40
    assert len(payment.ephemeral_pubkey) == 64


def test_two_payments_to_same_recipient_different_addresses():
    bob_keys = StealthKeys.generate()
    payment1 = generate_stealth_payment(bob_keys.spend_public_hex())
    payment2 = generate_stealth_payment(bob_keys.spend_public_hex())

    assert payment1.stealth_address != payment2.stealth_address
    assert payment1.ephemeral_pubkey != payment2.ephemeral_pubkey


def test_recipient_finds_own_payment():
    bob_keys = StealthKeys.generate()
    payment = generate_stealth_payment(bob_keys.spend_public_hex())

    found = scan_for_payment(
        spend_private_hex=bob_keys.spend_private_hex(),
        spend_public_hex=bob_keys.spend_public_hex(),
        ephemeral_pubkey_hex=payment.ephemeral_pubkey,
    )

    assert found == payment.stealth_address


def test_wrong_recipient_cannot_find_payment():
    bob_keys = StealthKeys.generate()
    alice_keys = StealthKeys.generate()

    payment = generate_stealth_payment(bob_keys.spend_public_hex())

    found = scan_for_payment(
        spend_private_hex=alice_keys.spend_private_hex(),
        spend_public_hex=alice_keys.spend_public_hex(),
        ephemeral_pubkey_hex=payment.ephemeral_pubkey,
    )

    assert found != payment.stealth_address


def test_stealth_payment_is_deterministic():
    bob_keys = StealthKeys.generate()
    payment = generate_stealth_payment(bob_keys.spend_public_hex())

    found1 = scan_for_payment(
        bob_keys.spend_private_hex(),
        bob_keys.spend_public_hex(),
        payment.ephemeral_pubkey,
    )
    found2 = scan_for_payment(
        bob_keys.spend_private_hex(),
        bob_keys.spend_public_hex(),
        payment.ephemeral_pubkey,
    )

    assert found1 == found2 == payment.stealth_address


def test_stealth_transaction_in_node():
    node = Node()
    alice = Wallet.generate()
    bob_keys = StealthKeys.generate()

    node.bootstrap_genesis(alice.address, GENESIS_BALANCE)

    payment = generate_stealth_payment(bob_keys.spend_public_hex())

    node.state.ensure_account(payment.stealth_address)

    import time
    from app.ledger.transaction import TransactionVertex
    from app.config import ANTI_SPAM_DIFFICULTY

    nonce = node.state.get_nonce(alice.address) + 1
    tx = TransactionVertex(
        sender=alice.address,
        receiver=payment.stealth_address,
        amount=100,
        nonce=nonce,
        timestamp=int(time.time()),
        public_key=alice.public_key,
        parents=node.select_parents(),
        ephemeral_pubkey=payment.ephemeral_pubkey,
    )

    nonce_val = 0
    while True:
        tx.anti_spam_nonce = nonce_val
        tx.anti_spam_hash = tx.compute_anti_spam_hash()
        if tx.anti_spam_hash.startswith("0" * ANTI_SPAM_DIFFICULTY):
            break
        nonce_val += 1

    tx.signature = alice.sign(tx.signing_payload())
    tx.finalize()

    result = node.submit_transaction(tx)
    assert result.ok

    found_payments = []
    for dag_tx in node.dag.vertices.values():
        if not dag_tx.ephemeral_pubkey:
            continue
        stealth_addr = scan_for_payment(
            bob_keys.spend_private_hex(),
            bob_keys.spend_public_hex(),
            dag_tx.ephemeral_pubkey,
        )
        if stealth_addr and node.state.balances.get(stealth_addr, 0) > 0:
            found_payments.append((stealth_addr, node.state.balances[stealth_addr]))

    assert len(found_payments) == 1
    assert found_payments[0][1] == 100