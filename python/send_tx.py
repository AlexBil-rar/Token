# send_tx.py

import sys
import httpx
import nacl.signing, nacl.encoding
import secrets
import time

from app.crypto.wallet import Wallet
from app.crypto.hashing import sha256_hex
from app.ledger.transaction import TransactionVertex
from app.config import ANTI_SPAM_DIFFICULTY

ALICE_ADDRESS  = sys.argv[1]
ALICE_PRIV_KEY = sys.argv[2]

signing_key = nacl.signing.SigningKey(
    ALICE_PRIV_KEY.encode(), encoder=nacl.encoding.HexEncoder
)
public_key = signing_key.verify_key.encode(nacl.encoding.HexEncoder).decode()
alice = Wallet(private_key=ALICE_PRIV_KEY, public_key=public_key, address=ALICE_ADDRESS)

bob_address = sha256_hex(secrets.token_hex(32).encode())[:40]

tx = TransactionVertex(
    sender=alice.address,
    receiver=bob_address,
    amount=100,
    nonce=1,
    timestamp=int(time.time()),
    public_key=alice.public_key,
    parents=[],
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

print(f"Sending tx {tx.tx_id[:16]}... to node 8000")

response = httpx.post(
    "http://127.0.0.1:8000/receive_transaction",
    json=tx.to_dict(),
)
print("Node 8000 response:", response.json())

time.sleep(0.5)
dag_b = httpx.get("http://127.0.0.1:8001/dag").json()
print(f"\nNode 8001 DAG stats: {dag_b['stats']}")
print(f"TX in node 8001: {tx.tx_id in dag_b['transactions']}")