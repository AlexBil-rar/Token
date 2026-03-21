# app/config.py

# --- const ---
MIN_STAKE = 1_000           # минимальный залог для участия в сети
SLASH_PERCENT = 0.10        # 10% от stake за каждое нарушение
SLASH_BURN_RATIO = 0.50     # 50% slash идёт на сжигание (дефляция)
SLASH_POOL_RATIO = 0.50     # 50% slash идёт честным узлам
MAX_VIOLATIONS = 3          # после 3 нарушений — исключение из сети


# --- const ---
REGISTRATION_POW_DIFFICULTY = 5       # сложность PoW при регистрации узла
MIN_REPUTATION = 0.0
MAX_REPUTATION = 1.0
REPUTATION_GROWTH_PER_HOUR = 0.01     # +1% репутации за каждый час онлайн
REPUTATION_PENALTY = 0.20             # -20% за плохое поведение
REPUTATION_FULL_WEEKS = 1             # полная репутация через 1 неделю
MAX_NODES_PER_IP = 3                  # максимум узлов с одного IP
BEHAVIOUR_WINDOW = 100                # последние N голосований для анализа
BEHAVIOUR_AGREEMENT_THRESHOLD = 0.95  # если согласен >95% времени — подозрительно

# --- const ---
TOTAL_SUPPLY = 21_000_000       
GENESIS_SHARE = 0.10            
ADDRESS_CAP = 0.001            
BASE_REWARD_PER_HOUR = 10       

HALVENING_INTERVAL = 4 * 365 * 24 * 3600

UPTIME_TIERS = [
    (24 * 3600,        1.00),   
    (72 * 3600,        0.50),   
    (168 * 3600,       0.25),   
    (float("inf"),     0.10),
]

PRUNING_WINDOW = 10_000     
PRUNING_INTERVAL = 1_000    

ANTI_SPAM_DIFFICULTY = 3
CONFIRMATION_THRESHOLD = 6
GENESIS_ADDRESS = "GENESIS"
GENESIS_BALANCE = 10_000_000
MAX_PARENTS = 2