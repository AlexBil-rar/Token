# tests/test_websocket.py

import asyncio
import json
import pytest
from app.network.ws_manager import WSConnectionManager, WSMessage, MessageType


def test_message_to_json():
    msg = WSMessage(type=MessageType.PING, payload={})
    data = json.loads(msg.to_json())
    assert data["type"] == "ping"
    assert "timestamp" in data


def test_message_from_json():
    raw = json.dumps({
        "type": "transaction",
        "payload": {"tx_id": "abc123"},
        "timestamp": 1000.0,
        "sender": "node1",
    })
    msg = WSMessage.from_json(raw)
    assert msg.type == MessageType.TRANSACTION
    assert msg.payload["tx_id"] == "abc123"
    assert msg.sender == "node1"


def test_message_roundtrip():
    msg = WSMessage(
        type=MessageType.PEER_LIST,
        payload={"peers": ["ws://1.2.3.4:9000"]},
        sender="node_a",
    )
    restored = WSMessage.from_json(msg.to_json())
    assert restored.type == msg.type
    assert restored.payload == msg.payload
    assert restored.sender == msg.sender


def test_all_message_types():
    for mt in MessageType:
        msg = WSMessage(type=mt, payload={})
        restored = WSMessage.from_json(msg.to_json())
        assert restored.type == mt


class MockWebSocket:
    def __init__(self):
        self.sent = []
        self.closed = False

    async def send(self, data: str) -> None:
        if self.closed:
            raise Exception("connection closed")
        self.sent.append(data)


def test_manager_register_and_unregister():
    manager = WSConnectionManager()
    ws = MockWebSocket()

    manager.register("peer1", ws)
    assert manager.is_connected("peer1")

    manager.unregister("peer1")
    assert not manager.is_connected("peer1")


def test_manager_get_active_peers():
    manager = WSConnectionManager()
    manager.register("peer1", MockWebSocket())
    manager.register("peer2", MockWebSocket())

    peers = manager.get_active_peers()
    assert "peer1" in peers
    assert "peer2" in peers


@pytest.mark.asyncio
async def test_manager_send():
    manager = WSConnectionManager()
    ws = MockWebSocket()
    manager.register("peer1", ws)

    msg = WSMessage(type=MessageType.PING, payload={})
    result = await manager.send("peer1", msg)

    assert result is True
    assert len(ws.sent) == 1
    assert json.loads(ws.sent[0])["type"] == "ping"


@pytest.mark.asyncio
async def test_manager_send_unknown_peer():
    manager = WSConnectionManager()
    msg = WSMessage(type=MessageType.PING, payload={})
    result = await manager.send("unknown", msg)
    assert result is False


@pytest.mark.asyncio
async def test_manager_send_closed_connection():
    manager = WSConnectionManager()
    ws = MockWebSocket()
    ws.closed = True
    manager.register("peer1", ws)

    msg = WSMessage(type=MessageType.PING, payload={})
    result = await manager.send("peer1", msg)

    assert result is False
    assert not manager.is_connected("peer1")


@pytest.mark.asyncio
async def test_manager_broadcast():
    manager = WSConnectionManager()
    ws1 = MockWebSocket()
    ws2 = MockWebSocket()
    ws3 = MockWebSocket()

    manager.register("peer1", ws1)
    manager.register("peer2", ws2)
    manager.register("peer3", ws3)

    msg = WSMessage(type=MessageType.PING, payload={})
    results = await manager.broadcast(msg, exclude="peer2")

    assert results["peer1"] is True
    assert results["peer3"] is True
    assert "peer2" not in results
    assert len(ws2.sent) == 0


@pytest.mark.asyncio
async def test_manager_broadcast_all():
    manager = WSConnectionManager()
    ws1 = MockWebSocket()
    ws2 = MockWebSocket()
    manager.register("peer1", ws1)
    manager.register("peer2", ws2)

    msg = WSMessage(type=MessageType.TRANSACTION, payload={"tx_id": "abc"})
    results = await manager.broadcast(msg)

    assert results["peer1"] is True
    assert results["peer2"] is True
    assert len(ws1.sent) == 1
    assert len(ws2.sent) == 1


def test_manager_stats():
    manager = WSConnectionManager()
    manager.register("peer1", MockWebSocket())
    manager.register("peer2", MockWebSocket())

    s = manager.stats()
    assert s["active_connections"] == 2
    assert "peer1" in s["peers"]
    assert "peer2" in s["peers"]