-- Shuns (silent bans - user stays connected but commands are ignored)

CREATE TABLE shuns (
    mask TEXT PRIMARY KEY,
    reason TEXT,
    set_by TEXT NOT NULL,
    set_at INTEGER NOT NULL,
    expires_at INTEGER
);

-- G-Lines (global hostmask bans)
CREATE TABLE glines (
    mask TEXT PRIMARY KEY,
    reason TEXT,
    set_by TEXT NOT NULL,
    set_at INTEGER NOT NULL,
    expires_at INTEGER
);

-- Z-Lines (IP bans that skip DNS)
CREATE TABLE zlines (
    mask TEXT PRIMARY KEY,
    reason TEXT,
    set_by TEXT NOT NULL,
    set_at INTEGER NOT NULL,
    expires_at INTEGER
);
