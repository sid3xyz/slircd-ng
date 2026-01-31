-- Metadata Persistence Migration

-- Metadata for registered accounts (NickServ)
CREATE TABLE account_metadata (
    account_id INTEGER NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (account_id, key),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

-- Metadata for registered channels (ChanServ)
CREATE TABLE channel_metadata (
    channel_id INTEGER NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (channel_id, key),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON DELETE CASCADE
);

-- Metadata for runtime channel state (Actor Persistence)
ALTER TABLE channel_state ADD COLUMN metadata TEXT;
