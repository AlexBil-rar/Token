# send_tx.py
import httpx
from app.crypto.wallet import Wallet
from app.ledger.node import Node

# вставь свои данные из терминала
ALICE_ADDRESS   = "46cdcc763d75646235eb3b4bdc00bf72ef63eb37"
ALICE_PRIV_KEY  = "060502d7ca65f8abec729c299b737eed1e68b54da434cc418d73d22466984d44"

# восстанавливаем кошелёк из приватного ключа
import nacl.signing, nacl.encoding
from app.crypto.hashing import sha256_hex

signing_key = nacl.signing.SigningKey(
    ALICE_PRIV_KEY.encode(), encoder=nacl.encoding.HexEncoder
)
public_key = signing_key.verify_key.encode(nacl.encoding.HexEncoder).decode()
alice = Wallet(private_key=ALICE_PRIV_KEY, public_key=public_key, address=ALICE_ADDRESS)

# создаём получателя
import secrets
bob_address = sha256_hex(secrets.token_hex(32).encode())[:40]

# собираем транзакцию вручную
import time
from app.ledger.transaction import TransactionVertex

tx = TransactionVertex(
    sender=alice.address,
    receiver=bob_address,
    amount=100,
    nonce=1,
    timestamp=int(time.time()),
    public_key=alice.public_key,
    parents=[],
)

# mine anti-spam
from app.config import ANTI_SPAM_DIFFICULTY
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

# проверяем что узел 8001 тоже получил
import time; time.sleep(0.5)
dag_b = httpx.get("http://127.0.0.1:8001/dag").json()
print(f"\nNode 8001 DAG stats: {dag_b['stats']}")
print(f"TX in node 8001: {tx.tx_id in dag_b['transactions']}")