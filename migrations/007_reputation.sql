-- Reputation system table
CREATE TABLE IF NOT EXISTS reputation (
    entity TEXT PRIMARY KEY,          -- "account:name" or "ip:hash"
    trust_score INTEGER NOT NULL DEFAULT 0,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    connections INTEGER NOT NULL DEFAULT 0,
    violations INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_reputation_trust ON reputation(trust_score);
