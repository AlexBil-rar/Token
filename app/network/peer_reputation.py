# app/network/peer_reputation.py

from __future__ import annotations

import hashlib
import time
from collections import defaultdict
from dataclasses import dataclass, field

from app.config import(
REGISTRATION_POW_DIFFICULTY,
MIN_REPUTATION,
MAX_REPUTATION,
REPUTATION_GROWTH_PER_HOUR,   
REPUTATION_PENALTY,           
REPUTATION_FULL_WEEKS,             
MAX_NODES_PER_IP,                  
BEHAVIOUR_WINDOW,                
BEHAVIOUR_AGREEMENT_THRESHOLD,
)

@dataclass
class PeerRecord:
    address: str          # http://ip:port
    ip: str
    registered_at: float = field(default_factory=time.time)
    last_seen: float = field(default_factory=time.time)
    reputation: float = 0.1           
    is_banned: bool = False
    ban_reason: str = ""
    vote_history: list[str] = field(default_factory=list)  
    penalties: int = 0

    def uptime_hours(self, now: float | None = None) -> float:
        now = now or time.time()
        return (now - self.registered_at) / 3600

    def update_reputation(self, now: float | None = None) -> None:
        now = now or time.time()
        hours = self.uptime_hours(now)
        target = min(hours / (REPUTATION_FULL_WEEKS * 7 * 24), MAX_REPUTATION)
        self.reputation = round(target, 4)

    def add_vote(self, vote_hash: str) -> None:
        self.vote_history.append(vote_hash)
        if len(self.vote_history) > BEHAVIOUR_WINDOW:
            self.vote_history.pop(0)

    def behaviour_score(self) -> float:
        if len(self.vote_history) < 10:
            return 1.0

        from collections import Counter
        counts = Counter(self.vote_history)
        most_common_count = counts.most_common(1)[0][1]
        agreement_rate = most_common_count / len(self.vote_history)

        if agreement_rate >= BEHAVIOUR_AGREEMENT_THRESHOLD:
            return 0.1  
        return 1.0

    def effective_weight(self) -> float:
        if self.is_banned:
            return 0.0
        return round(self.reputation * self.behaviour_score(), 4)


class PeerReputation:
    def __init__(self) -> None:
        self.peers: dict[str, PeerRecord] = {}
        self.ip_count: dict[str, int] = defaultdict(int)


    def generate_registration_challenge(self, address: str) -> str:
        return hashlib.sha256(f"register:{address}:{time.time()}".encode()).hexdigest()

    def verify_registration_pow(
        self, address: str, challenge: str, nonce: int
    ) -> bool:
        attempt = hashlib.sha256(
            f"{challenge}{nonce}".encode()
        ).hexdigest()
        return attempt.startswith("0" * REGISTRATION_POW_DIFFICULTY)

    def solve_registration_pow(self, challenge: str) -> int:
        nonce = 0
        while True:
            attempt = hashlib.sha256(
                f"{challenge}{nonce}".encode()
            ).hexdigest()
            if attempt.startswith("0" * REGISTRATION_POW_DIFFICULTY):
                return nonce
            nonce += 1

    def register_peer(
        self,
        address: str,
        ip: str,
        challenge: str,
        nonce: int,
        now: float | None = None,
    ) -> tuple[bool, str]:
        now = now or time.time()

        if not self.verify_registration_pow(address, challenge, nonce):
            return False, "invalid_pow"

        if self.ip_count[ip] >= MAX_NODES_PER_IP:
            return False, "too_many_nodes_from_ip"

        if address in self.peers:
            return False, "already_registered"

        self.peers[address] = PeerRecord(
            address=address,
            ip=ip,
            registered_at=now,
            last_seen=now,
        )
        self.ip_count[ip] += 1
        return True, "registered"

    def ping_peer(self, address: str, now: float | None = None) -> None:
        now = now or time.time()
        if address in self.peers:
            self.peers[address].last_seen = now
            self.peers[address].update_reputation(now)

    def record_vote(self, address: str, vote_hash: str) -> None:
        if address in self.peers:
            self.peers[address].add_vote(vote_hash)

    def ban_peer(self, address: str, reason: str) -> None:
        if address in self.peers:
            self.peers[address].is_banned = True
            self.peers[address].ban_reason = reason
            ip = self.peers[address].ip
            self.ip_count[ip] = max(0, self.ip_count[ip] - 1)

    def penalize_peer(self, address: str) -> None:
        if address in self.peers:
            peer = self.peers[address]
            peer.reputation = max(
                MIN_REPUTATION,
                peer.reputation - REPUTATION_PENALTY
            )
            peer.penalties += 1
            if peer.penalties >= 3:
                self.ban_peer(address, "too_many_penalties")

    def get_trusted_peers(self, min_reputation: float = 0.3) -> list[PeerRecord]:
        return [
            p for p in self.peers.values()
            if not p.is_banned and p.reputation >= min_reputation
        ]

    def get_quorum_weights(self) -> dict[str, float]:
        return {
            address: peer.effective_weight()
            for address, peer in self.peers.items()
            if not peer.is_banned
        }

    def stats(self) -> dict:
        total = len(self.peers)
        banned = sum(1 for p in self.peers.values() if p.is_banned)
        trusted = len(self.get_trusted_peers())
        avg_reputation = (
            sum(p.reputation for p in self.peers.values()) / total
            if total > 0 else 0
        )
        return {
            "total_peers": total,
            "banned": banned,
            "trusted": trusted,
            "average_reputation": round(avg_reputation, 4),
        }