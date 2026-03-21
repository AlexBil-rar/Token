# tests/test_token.py

import time
from app.token.ghost import (
    GhostToken,
    TOTAL_SUPPLY,
    ADDRESS_CAP,
    BASE_REWARD_PER_HOUR,
)


def test_genesis_distributes_to_founder():
    token = GhostToken()
    amount = token.genesis("founder")
    assert amount > 0
    assert token.get_balance("founder") == amount


def test_genesis_only_once():
    token = GhostToken()
    token.genesis("founder")
    second = token.genesis("founder2")
    assert second == 0


def test_genesis_respects_address_cap():
    token = GhostToken()
    amount = token.genesis("founder")
    cap = int(TOTAL_SUPPLY * ADDRESS_CAP)
    assert amount <= cap


def test_register_node():
    token = GhostToken()
    token.register_node("node1")
    assert "node1" in token.nodes


def test_ping_node_registers_if_unknown():
    token = GhostToken()
    token.ping_node("node1")
    assert "node1" in token.nodes


def test_ping_resets_streak_after_gap():
    now = time.time()
    token = GhostToken()
    token.register_node("node1", now=now)

    token.ping_node("node1", now=now + 3 * 3600)
    assert token.nodes["node1"].continuous_since == now + 3 * 3600


def test_ping_keeps_streak_within_gap():
    now = time.time()
    token = GhostToken()
    token.register_node("node1", now=now)
    original_since = token.nodes["node1"].continuous_since

    token.ping_node("node1", now=now + 3600)
    assert token.nodes["node1"].continuous_since == original_since


def test_multiplier_first_day():
    token = GhostToken()
    m = token._uptime_multiplier(12 * 3600) 
    assert m == 1.0


def test_multiplier_second_day():
    token = GhostToken()
    m = token._uptime_multiplier(48 * 3600) 
    assert m == 0.5


def test_multiplier_week():
    token = GhostToken()
    m = token._uptime_multiplier(100 * 3600)
    assert m == 0.25


def test_multiplier_over_week():
    token = GhostToken()
    m = token._uptime_multiplier(200 * 3600)
    assert m == 0.10


def test_claim_reward_fresh_node():
    now = time.time()
    token = GhostToken()
    token.register_node("node1", now=now)

    reward = token.claim_reward("node1", now=now + 3600)
    assert reward > 0
    assert token.get_balance("node1") == reward


def test_claim_reward_caps_at_address_limit():
    now = time.time()
    token = GhostToken()
    token.register_node("whale", now=now)

    total = 0
    for i in range(10000):
        total += token.claim_reward("whale", now=now + i * 3600)

    cap = int(TOTAL_SUPPLY * ADDRESS_CAP)
    assert token.get_balance("whale") <= cap


def test_claim_reward_diminishes_over_time():
    now = time.time()
    token = GhostToken()

    token.register_node("node_fresh", now=now)
    token.register_node("node_old", now=now - 200 * 3600)

    reward_fresh = token.claim_reward("node_fresh", now=now + 3600)
    reward_old = token.claim_reward("node_old", now=now + 3600)

    assert reward_fresh > reward_old


def test_claim_reward_respects_total_supply():
    token = GhostToken()
    token.total_minted = TOTAL_SUPPLY - 1
    token.register_node("node1")

    reward = token.claim_reward("node1")
    assert token.total_minted <= TOTAL_SUPPLY


def test_claim_reward_unknown_node_returns_zero():
    token = GhostToken()
    reward = token.claim_reward("unknown")
    assert reward == 0


def test_stats_structure():
    token = GhostToken()
    token.genesis("founder")
    token.register_node("node1")

    s = token.stats()
    assert s["total_supply"] == TOTAL_SUPPLY
    assert s["total_minted"] > 0
    assert s["supply_remaining"] < TOTAL_SUPPLY
    assert s["active_nodes"] == 1


def test_supply_percentage():
    token = GhostToken()
    token.genesis("founder")
    assert 0 < token.supply_percentage() < 100


def test_halvening_reduces_reward():
    now = time.time()
    four_years = 4 * 365 * 24 * 3600

    token_early = GhostToken(network_start=now)
    token_late = GhostToken(network_start=now - four_years)

    token_early.register_node("node", now=now)
    token_late.register_node("node", now=now)

    reward_early = token_early.claim_reward("node", now=now + 3600)
    reward_late = token_late.claim_reward("node", now=now + 3600)

    assert reward_early > reward_late