# run_node.py

import sys
import asyncio
import httpx
import uvicorn
from contextlib import asynccontextmanager
from pathlib import Path

from app.config import GENESIS_BALANCE
from app.crypto.wallet import Wallet
from app.ledger.node import Node
from app.network.peer_list import PeerList
from app.network.server import NodeServer
from app.storage.snapshot import SnapshotStorage
from fastapi import FastAPI


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8000

    snapshot_path = Path(f"data/node_{port}_snapshot.json")
    storage = SnapshotStorage(path=snapshot_path)

    node = Node(storage=storage)
    peers = PeerList()

    loaded = node.load_snapshot()

    if not loaded and port == 8000:
        wallet = Wallet.generate()
        node.bootstrap_genesis(wallet.address, GENESIS_BALANCE)
        node.storage.save(node.dag, node.state, wallet_data={
            "address": wallet.address,
            "private_key": wallet.private_key,
        })
        print(f"Genesis wallet address:     {wallet.address}")
        print(f"Genesis wallet private_key: {wallet.private_key}")

        # при загрузке snapshot — показываем сохранённый кошелёк
        wallet_data = node.storage.load(node.dag, node.state)
        if wallet_data:
            print(f"Resumed from snapshot — {len(node.dag.vertices)} transactions in DAG")
            print(f"Genesis wallet address:     {wallet_data['address']}")
            print(f"Genesis wallet private_key: {wallet_data['private_key']}")


    other_port = 8001 if port == 8000 else 8000
    peers.add(f"http://127.0.0.1:{other_port}")

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        if port == 8001 and not loaded:
            await asyncio.sleep(1.5)
            try:
                async with httpx.AsyncClient() as client:
                    resp = await client.get(
                        "http://127.0.0.1:8000/state", timeout=3
                    )
                    balances = resp.json().get("balances", {})
                    for address, balance in balances.items():
                        node.state.balances[address] = balance
                        node.state.ensure_account(address)
                node.save_snapshot()
                print("State synced from node 8000 ✓")
            except Exception as e:
                print(f"Sync failed: {e}")
        yield

    server = NodeServer(node=node, peers=peers, lifespan=lifespan)

    print(f"\nNode running at http://127.0.0.1:{port}")
    print(f"Peers: {peers.get_all()}\n")

    uvicorn.run(server.app, host="127.0.0.1", port=port)


if __name__ == "__main__":
    main()