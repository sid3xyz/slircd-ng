-- Phase 3: Extended X-lines (G-Lines and Z-Lines)
-- K-lines and D-lines already exist in 001_init.sql

-- G-Lines (Global hostmask bans - network-wide)
CREATE TABLE glines (
    mask TEXT PRIMARY KEY,
    reason TEXT,
    set_by TEXT NOT NULL,
    set_at INTEGER NOT NULL,
    expires_at INTEGER
);

-- Z-Lines (IP bans that skip DNS lookup - for performance)
CREATE TABLE zlines (
    mask TEXT PRIMARY KEY,
    reason TEXT,
    set_by TEXT NOT NULL,
    set_at INTEGER NOT NULL,
    expires_at INTEGER
);

-- Create indexes for expiry-based queries
CREATE INDEX idx_glines_expires ON glines(expires_at);
CREATE INDEX idx_zlines_expires ON zlines(expires_at);
CREATE INDEX idx_klines_expires ON klines(expires_at);
CREATE INDEX idx_dlines_expires ON dlines(expires_at);
