-- Shuns (silent bans - user stays connected but commands are ignored)

CREATE TABLE shuns (
    mask TEXT PRIMARY KEY,
    reason TEXT,
    set_by TEXT NOT NULL,
    set_at INTEGER NOT NULL,
    expires_at INTEGER
);
