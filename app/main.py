from __future__ import annotations

from pprint import pprint

from app.config import GENESIS_BALANCE
from app.crypto.wallet import Wallet
from app.ledger.node import Node


def main() -> None:
    node = Node()

    alice = Wallet.generate()
    bob = Wallet.generate()

    node.bootstrap_genesis(alice.address, GENESIS_BALANCE)

    print("=== wallets ===")
    print("alice:", alice.address)
    print("bob:  ", bob.address)

    print("\n=== initial state ===")
    pprint(node.get_state_view())

    tx1 = node.create_transaction(alice, bob.address, 100)
    result1 = node.submit_transaction(tx1)

    tx2 = node.create_transaction(alice, bob.address, 50)
    result2 = node.submit_transaction(tx2)

    tx3 = node.create_transaction(alice, bob.address, 30)
    result3 = node.submit_transaction(tx3)

    print("\n=== results ===")
    pprint(result1)
    pprint(result2)
    pprint(result3)

    print("\n=== state after txs ===")
    pprint(node.get_state_view())

    print("\n=== dag ===")
    pprint(node.get_dag_view())


if __name__ == "__main__":
    main()