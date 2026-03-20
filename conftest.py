# conftest.py
import pytest
from app.ledger.node import Node

@pytest.fixture
def node():
    return Node()