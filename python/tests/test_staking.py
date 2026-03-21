# tests/test_staking.py

from app.token.staking import (
    StakingManager,
    ViolationType,
    StakeStatus,
    MIN_STAKE,
    SLASH_PERCENT,
    MAX_VIOLATIONS,
)


def make_balances(address: str, amount: int) -> dict:
    return {address: amount}


def test_stake_success():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    ok, reason = manager.stake("node1", MIN_STAKE, balances)
    assert ok
    assert balances["node1"] == 5000 - MIN_STAKE
    assert manager.stakes["node1"].amount == MIN_STAKE


def test_stake_below_minimum():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    ok, reason = manager.stake("node1", MIN_STAKE - 1, balances)
    assert not ok
    assert "minimum" in reason


def test_stake_insufficient_balance():
    manager = StakingManager()
    balances = make_balances("node1", 500)
    ok, reason = manager.stake("node1", MIN_STAKE, balances)
    assert not ok
    assert "insufficient" in reason


def test_stake_duplicate():
    manager = StakingManager()
    balances = make_balances("node1", 10000)
    manager.stake("node1", MIN_STAKE, balances)
    ok, reason = manager.stake("node1", MIN_STAKE, balances)
    assert not ok
    assert "already" in reason


def test_slash_reduces_stake():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    manager.stake("node1", MIN_STAKE, balances)

    result = manager.slash("node1", ViolationType.DOUBLE_VOTE)

    expected_slash = int(MIN_STAKE * SLASH_PERCENT)
    assert result.slashed_amount == expected_slash
    assert manager.stakes["node1"].amount == MIN_STAKE - expected_slash


def test_slash_splits_burned_and_pool():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    manager.stake("node1", MIN_STAKE, balances)

    result = manager.slash("node1", ViolationType.DOUBLE_VOTE)

    assert result.burned + result.to_pool == result.slashed_amount
    assert manager.total_burned == result.burned
    assert manager.slash_pool == result.to_pool


def test_slash_records_violation():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    manager.stake("node1", MIN_STAKE, balances)

    manager.slash("node1", ViolationType.DOUBLE_VOTE, evidence="block_hash_123")

    record = manager.stakes["node1"]
    assert record.violation_count() == 1
    assert record.violations[0]["type"] == "double_vote"
    assert record.violations[0]["evidence"] == "block_hash_123"


def test_slash_ejection_after_max_violations():
    manager = StakingManager()
    balances = make_balances("node1", 10000)
    manager.stake("node1", MIN_STAKE, balances)

    for i in range(MAX_VIOLATIONS):
        result = manager.slash("node1", ViolationType.DOUBLE_VOTE)

    assert result.ejected
    assert manager.stakes["node1"].status == StakeStatus.EJECTED
    assert manager.stakes["node1"].amount == 0


def test_slash_ejected_node_burns_remaining():
    """При исключении весь остаток stake сжигается или идёт в pool."""
    manager = StakingManager()
    balances = make_balances("node1", 10000)
    manager.stake("node1", MIN_STAKE, balances)

    burned_before = manager.total_burned
    for _ in range(MAX_VIOLATIONS):
        manager.slash("node1", ViolationType.DOUBLE_VOTE)

    assert manager.total_burned > burned_before
    assert manager.stakes["node1"].amount == 0


def test_slash_unknown_node_returns_none():
    manager = StakingManager()
    result = manager.slash("unknown", ViolationType.DOUBLE_VOTE)
    assert result is None


def test_withdraw_returns_stake():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    manager.stake("node1", MIN_STAKE, balances)

    ok, reason, amount = manager.withdraw("node1", balances)

    assert ok
    assert amount == MIN_STAKE
    assert balances["node1"] == 5000


def test_withdraw_after_slash_returns_reduced():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    manager.stake("node1", MIN_STAKE, balances)
    manager.slash("node1", ViolationType.DOUBLE_VOTE)

    ok, reason, amount = manager.withdraw("node1", balances)

    assert ok
    expected = int(MIN_STAKE * (1 - SLASH_PERCENT))
    assert amount == expected


def test_withdraw_ejected_node_fails():
    manager = StakingManager()
    balances = make_balances("node1", 10000)
    manager.stake("node1", MIN_STAKE, balances)

    for _ in range(MAX_VIOLATIONS):
        manager.slash("node1", ViolationType.DOUBLE_VOTE)

    ok, reason, amount = manager.withdraw("node1", balances)
    assert not ok
    assert amount == 0


def test_withdraw_twice_fails():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    manager.stake("node1", MIN_STAKE, balances)
    manager.withdraw("node1", balances)

    ok, reason, amount = manager.withdraw("node1", balances)
    assert not ok


def test_distribute_slash_pool():
    manager = StakingManager()

    balances = {"node1": 5000, "node2": 5000, "bad": 5000}
    manager.stake("node1", MIN_STAKE, balances)
    manager.stake("node2", MIN_STAKE, balances)
    manager.stake("bad", MIN_STAKE, balances)

    manager.slash("bad", ViolationType.DOUBLE_VOTE)

    balance_before_1 = balances.get("node1", 0)
    balance_before_2 = balances.get("node2", 0)

    distributed = manager.distribute_slash_pool(balances)

    assert distributed > 0
    assert balances["node1"] > balance_before_1
    assert balances["node2"] > balance_before_2


def test_distribute_empty_pool():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    manager.stake("node1", MIN_STAKE, balances)

    distributed = manager.distribute_slash_pool(balances)
    assert distributed == 0


def test_eligible_active_staker():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    manager.stake("node1", MIN_STAKE, balances)
    assert manager.is_eligible("node1")


def test_not_eligible_without_stake():
    manager = StakingManager()
    assert not manager.is_eligible("node1")


def test_not_eligible_ejected():
    manager = StakingManager()
    balances = make_balances("node1", 10000)
    manager.stake("node1", MIN_STAKE, balances)
    for _ in range(MAX_VIOLATIONS):
        manager.slash("node1", ViolationType.DOUBLE_VOTE)
    assert not manager.is_eligible("node1")


def test_stake_weight_full_for_clean_node():
    manager = StakingManager()
    balances = make_balances("node1", 5000)
    manager.stake("node1", MIN_STAKE, balances)
    assert manager.get_stake_weight("node1") == 1.0


def test_stake_weight_reduced_after_slash():
    manager = StakingManager()
    balances = make_balances("node1", 10000)
    manager.stake("node1", MIN_STAKE * 2, balances)
    manager.slash("node1", ViolationType.DOUBLE_VOTE)
    weight = manager.get_stake_weight("node1")
    assert weight < 1.0
    assert weight > 0.0


def test_stats():
    manager = StakingManager()
    balances = {"node1": 5000, "node2": 5000}
    manager.stake("node1", MIN_STAKE, balances)
    manager.stake("node2", MIN_STAKE, balances)

    s = manager.stats()
    assert s["total_stakers"] == 2
    assert s["active_stakers"] == 2
    assert s["total_staked"] == MIN_STAKE * 2
    assert s["slash_pool"] == 0
    assert s["total_burned"] == 0