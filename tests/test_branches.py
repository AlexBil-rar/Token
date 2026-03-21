# tests/test_branches.py

from app.branches.branch import Branch
from app.branches.branch_manager import BranchManager
from app.branches.coordinator import Coordinator
from app.config import GENESIS_BALANCE
from app.crypto.wallet import Wallet
from app.ledger.node import Node


def make_tx(node: Node, wallet: Wallet, receiver: str, amount: int):
    return node.create_transaction(wallet, receiver, amount)


def test_branch_accepts_transaction():
    branch = Branch(branch_id="A")
    alice = Wallet.generate()
    bob = Wallet.generate()
    branch.state.credit(alice.address, 1000)

    node = Node()
    node.state = branch.state
    node.dag = branch.dag
    tx = node.create_transaction(alice, bob.address, 100)

    result = branch.submit_transaction(tx)
    assert result.ok


def test_branch_rejects_insufficient_balance():
    branch = Branch(branch_id="A")
    alice = Wallet.generate()
    bob = Wallet.generate()
    branch.state.credit(alice.address, 10)

    node = Node()
    node.state = branch.state
    node.dag = branch.dag
    tx = node.create_transaction(alice, bob.address, 100)

    result = branch.submit_transaction(tx)
    assert not result.ok


def test_branch_snapshot_contains_vertices():
    branch = Branch(branch_id="A")
    alice = Wallet.generate()
    bob = Wallet.generate()
    branch.state.credit(alice.address, 1000)

    node = Node()
    node.state = branch.state
    node.dag = branch.dag
    tx = node.create_transaction(alice, bob.address, 100)
    branch.submit_transaction(tx)

    snap = branch.snapshot()
    assert tx.tx_id in snap["vertices"]
    assert snap["branch_id"] == "A"


def test_coordinator_merge_balances():
    branch_a = Branch(branch_id="A")
    branch_b = Branch(branch_id="B")

    branch_a.state.credit("alice", 500)
    branch_b.state.credit("bob", 300)

    coordinator = Coordinator()
    root = coordinator.merge([branch_a, branch_b])

    assert root.balances["alice"] == 500
    assert root.balances["bob"] == 300


def test_coordinator_takes_min_balance():
    branch_a = Branch(branch_id="A")
    branch_b = Branch(branch_id="B")

    branch_a.state.credit("alice", 1000)
    branch_b.state.credit("alice", 600)

    coordinator = Coordinator()
    root = coordinator.merge([branch_a, branch_b])

    assert root.balances["alice"] == 600


def test_coordinator_takes_max_nonce():
    branch_a = Branch(branch_id="A")
    branch_b = Branch(branch_id="B")

    branch_a.state.credit("alice", 1000)
    branch_a.state.nonces["alice"] = 3
    branch_b.state.credit("alice", 1000)
    branch_b.state.nonces["alice"] = 5

    coordinator = Coordinator()
    root = coordinator.merge([branch_a, branch_b])

    assert root.nonces["alice"] == 3


def test_coordinator_merge_count():
    coordinator = Coordinator()
    branch = Branch(branch_id="A")
    coordinator.merge([branch])
    coordinator.merge([branch])
    assert coordinator.merge_count == 2


def test_branch_manager_routes_deterministically():
    manager = BranchManager()
    manager.create_branch("A")
    manager.create_branch("B")

    branch1 = manager.get_least_loaded_branch()
    branch2 = manager.get_least_loaded_branch()

    assert branch1.branch_id == branch2.branch_id


def test_branch_manager_submit_updates_coordinator():
    manager = BranchManager()
    manager.create_branch("A")
    manager.create_branch("B")

    alice = Wallet.generate()
    bob = Wallet.generate()
    manager.credit(alice.address, GENESIS_BALANCE)

    node = Node()
    branch = manager.get_least_loaded_branch()
    node.state = branch.state
    node.dag = branch.dag
    tx = node.create_transaction(alice, bob.address, 100)

    result = manager.submit_transaction(tx)
    assert result.ok
    assert manager.coordinator.merge_count >= 1


def test_branch_manager_two_branches_independent():
    manager = BranchManager()
    manager.create_branch("A")
    manager.create_branch("B")

    alice = Wallet.generate()
    bob = Wallet.generate()
    manager.credit(alice.address, 1000)
    manager.credit(bob.address, 1000)

    stats = manager.get_stats()
    assert len(stats["branches"]) == 2