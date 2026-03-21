# tests/test_peer_reputation.py

import time
from app.network.peer_reputation import (
    PeerReputation,
    PeerRecord,
    MAX_NODES_PER_IP,
    BEHAVIOUR_AGREEMENT_THRESHOLD,
    REGISTRATION_POW_DIFFICULTY,
)


def register_peer(reputation: PeerReputation, address: str, ip: str = "1.2.3.4", now: float | None = None) -> bool:
    challenge = reputation.generate_registration_challenge(address)
    nonce = reputation.solve_registration_pow(challenge)
    ok, _ = reputation.register_peer(address, ip, challenge, nonce, now=now)
    return ok


def test_registration_requires_valid_pow():
    rep = PeerReputation()
    challenge = rep.generate_registration_challenge("node1")
    ok, reason = rep.register_peer("node1", "1.2.3.4", challenge, nonce=0)
    assert reason in ("invalid_pow", "registered")


def test_registration_with_solved_pow():
    rep = PeerReputation()
    ok = register_peer(rep, "node1")
    assert ok
    assert "node1" in rep.peers


def test_registration_rejects_duplicate():
    rep = PeerReputation()
    register_peer(rep, "node1")
    ok = register_peer(rep, "node1")
    assert not ok


def test_pow_difficulty():
    rep = PeerReputation()
    challenge = rep.generate_registration_challenge("test")
    nonce = rep.solve_registration_pow(challenge)
    import hashlib
    result = hashlib.sha256(f"{challenge}{nonce}".encode()).hexdigest()
    assert result.startswith("0" * REGISTRATION_POW_DIFFICULTY)


def test_ip_limit_blocks_too_many_nodes():
    rep = PeerReputation()
    for i in range(MAX_NODES_PER_IP):
        ok = register_peer(rep, f"node{i}", ip="1.2.3.4")
        assert ok

    ok = register_peer(rep, f"node{MAX_NODES_PER_IP}", ip="1.2.3.4")
    assert not ok


def test_different_ips_allowed():
    rep = PeerReputation()
    for i in range(10):
        ok = register_peer(rep, f"node{i}", ip=f"1.2.3.{i}")
        assert ok


def test_new_peer_has_low_reputation():
    rep = PeerReputation()
    register_peer(rep, "node1")
    assert rep.peers["node1"].reputation < 0.5


def test_reputation_grows_over_time():
    now = time.time()
    rep = PeerReputation()
    register_peer(rep, "node1", now=now)

    rep.ping_peer("node1", now=now + 3 * 24 * 3600)
    assert rep.peers["node1"].reputation > 0.3


def test_full_reputation_after_one_week():
    now = time.time()
    rep = PeerReputation()
    register_peer(rep, "node1", now=now)

    rep.ping_peer("node1", now=now + 7 * 24 * 3600)
    assert rep.peers["node1"].reputation >= 0.99


def test_suspicious_behaviour_reduces_weight():
    rep = PeerReputation()
    register_peer(rep, "node1")

    for _ in range(50):
        rep.record_vote("node1", "always_same_vote")

    peer = rep.peers["node1"]
    assert peer.behaviour_score() < 0.5
    assert peer.effective_weight() < peer.reputation


def test_varied_behaviour_is_normal():
    rep = PeerReputation()
    register_peer(rep, "node1")

    for i in range(50):
        rep.record_vote("node1", f"vote_{i}")

    peer = rep.peers["node1"]
    assert peer.behaviour_score() == 1.0


def test_insufficient_votes_trusts_peer():
    rep = PeerReputation()
    register_peer(rep, "node1")

    rep.record_vote("node1", "vote_1")
    rep.record_vote("node1", "vote_1")

    assert rep.peers["node1"].behaviour_score() == 1.0


def test_ban_peer():
    rep = PeerReputation()
    register_peer(rep, "node1", ip="1.2.3.4")
    rep.ban_peer("node1", "malicious")

    assert rep.peers["node1"].is_banned
    assert rep.peers["node1"].effective_weight() == 0.0


def test_ban_frees_ip_slot():
    rep = PeerReputation()
    register_peer(rep, "node1", ip="1.2.3.4")
    rep.ban_peer("node1", "malicious")

    ok = register_peer(rep, "node2", ip="1.2.3.4")
    assert ok


def test_three_penalties_cause_ban():
    rep = PeerReputation()
    register_peer(rep, "node1")

    rep.penalize_peer("node1")
    rep.penalize_peer("node1")
    assert not rep.peers["node1"].is_banned

    rep.penalize_peer("node1")
    assert rep.peers["node1"].is_banned


def test_get_trusted_peers():
    now = time.time()
    rep = PeerReputation()
    register_peer(rep, "node_old", now=now - 7 * 24 * 3600)
    register_peer(rep, "node_new", ip="2.3.4.5", now=now)

    rep.ping_peer("node_old", now=now)

    trusted = rep.get_trusted_peers(min_reputation=0.3)
    addresses = [p.address for p in trusted]
    assert "node_old" in addresses
    assert "node_new" not in addresses


def test_quorum_weights_excludes_banned():
    rep = PeerReputation()
    register_peer(rep, "node1")
    register_peer(rep, "node2", ip="2.3.4.5")
    rep.ban_peer("node1", "bad")

    weights = rep.get_quorum_weights()
    assert "node1" not in weights
    assert "node2" in weights


def test_stats():
    rep = PeerReputation()
    register_peer(rep, "node1")
    register_peer(rep, "node2", ip="2.3.4.5")
    rep.ban_peer("node1", "bad")

    s = rep.stats()
    assert s["total_peers"] == 2
    assert s["banned"] == 1