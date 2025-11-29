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

-- Phase 3: ChanServ Tables

-- Registered channels
CREATE TABLE channels (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL COLLATE NOCASE,
    founder_account_id INTEGER NOT NULL REFERENCES accounts(id),
    registered_at INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL,
    description TEXT,
    mlock TEXT,
    keeptopic BOOLEAN DEFAULT TRUE
);

-- Create index for founder lookups
CREATE INDEX idx_channels_founder ON channels(founder_account_id);

-- Channel access list
CREATE TABLE channel_access (
    channel_id INTEGER NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    flags TEXT NOT NULL,
    added_by TEXT NOT NULL,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (channel_id, account_id)
);

-- Create index for account access lookups
CREATE INDEX idx_channel_access_account ON channel_access(account_id);

-- Channel AKICK list
CREATE TABLE channel_akick (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    mask TEXT NOT NULL,
    reason TEXT,
    set_by TEXT NOT NULL,
    set_at INTEGER NOT NULL,
    UNIQUE(channel_id, mask)
);

-- Create index for channel akick lookups
CREATE INDEX idx_channel_akick_channel ON channel_akick(channel_id);
