# app/config.py

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