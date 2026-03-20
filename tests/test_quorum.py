# tests/test_quorum.py

from app.branches.branch import Branch
from app.branches.coordinator import Coordinator


def make_branch(branch_id: str, balances: dict, nonces: dict = None) -> Branch:
    branch = Branch(branch_id=branch_id)
    for address, balance in balances.items():
        branch.state.credit(address, balance)
    if nonces:
        branch.state.nonces.update(nonces)
    return branch


def test_quorum_size_odd():
    coord = Coordinator()
    assert coord._quorum_size(5) == 3  # 3 из 5
    assert coord._quorum_size(3) == 2  # 2 из 3


def test_quorum_size_even():
    coord = Coordinator()
    assert coord._quorum_size(4) == 3  # 3 из 4
    assert coord._quorum_size(2) == 2  # 2 из 2


def test_quorum_value_clear_winner():
    coord = Coordinator()
    votes = [900, 900, 900, 850, 800]
    assert coord._quorum_value(votes, quorum=3) == 900


def test_quorum_value_fallback_to_min():
    coord = Coordinator()
    votes = [900, 850, 800]
    assert coord._quorum_value(votes, quorum=2) == 800


def test_quorum_value_two_branches():
    coord = Coordinator()
    votes = [500, 500]
    assert coord._quorum_value(votes, quorum=2) == 500


def test_merge_quorum_majority_wins():
    branches = [
        make_branch("A", {"alice": 900}),
        make_branch("B", {"alice": 900}),
        make_branch("C", {"alice": 900}),
        make_branch("D", {"alice": 900}),
        make_branch("E", {"alice": 500}), 
    ]
    coord = Coordinator()
    root = coord.merge(branches)
    assert root.balances["alice"] == 900


def test_merge_quorum_not_reached_uses_min():
    branches = [
        make_branch("A", {"alice": 1000}),
        make_branch("B", {"alice": 800}),
        make_branch("C", {"alice": 600}),
    ]
    coord = Coordinator()
    root = coord.merge(branches)
    assert root.balances["alice"] == 600


def test_merge_two_branches_both_agree():
    branches = [
        make_branch("A", {"alice": 500}),
        make_branch("B", {"alice": 500}),
    ]
    coord = Coordinator()
    root = coord.merge(branches)
    assert root.balances["alice"] == 500


def test_merge_nonce_quorum():
    branches = [
        make_branch("A", {"alice": 900}, nonces={"alice": 3}),
        make_branch("B", {"alice": 900}, nonces={"alice": 3}),
        make_branch("C", {"alice": 900}, nonces={"alice": 5}),
    ]
    coord = Coordinator()
    root = coord.merge(branches)
    assert root.nonces["alice"] == 3


def test_has_quorum_true():
    branches = [
        make_branch("A", {"alice": 900}),
        make_branch("B", {"alice": 900}),
        make_branch("C", {"alice": 850}),
    ]
    coord = Coordinator()
    assert coord.has_quorum(branches, "alice") is True


def test_has_quorum_false():
    branches = [
        make_branch("A", {"alice": 900}),
        make_branch("B", {"alice": 800}),
        make_branch("C", {"alice": 700}),
    ]
    coord = Coordinator()
    assert coord.has_quorum(branches, "alice") is False


def test_merge_count_increments():
    coord = Coordinator()
    branch = make_branch("A", {"alice": 100})
    coord.merge([branch])
    coord.merge([branch])
    assert coord.merge_count == 2