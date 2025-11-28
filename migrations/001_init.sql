-- Initial schema for slircd-ng
-- Phase 2: NickServ accounts, nicknames, K-lines, D-lines

-- Accounts (NickServ)
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    email TEXT,
    registered_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    enforce BOOLEAN DEFAULT FALSE,
    hide_email BOOLEAN DEFAULT TRUE
);

-- Nicknames (linked to accounts)
CREATE TABLE nicknames (
    name TEXT PRIMARY KEY COLLATE NOCASE,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE
);

-- Create index for nickname lookups
CREATE INDEX idx_nicknames_account ON nicknames(account_id);

-- K-Lines (user@host bans)
CREATE TABLE klines (
    mask TEXT PRIMARY KEY,
    reason TEXT,
    set_by TEXT NOT NULL,
    set_at INTEGER NOT NULL,
    expires_at INTEGER
);

-- D-Lines (IP bans)
CREATE TABLE dlines (
    mask TEXT PRIMARY KEY,
    reason TEXT,
    set_by TEXT NOT NULL,
    set_at INTEGER NOT NULL,
    expires_at INTEGER
);
