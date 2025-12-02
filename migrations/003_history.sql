-- Message history for CHATHISTORY command (IRCv3 draft/chathistory)

CREATE TABLE IF NOT EXISTS message_history (
    msgid TEXT PRIMARY KEY,
    target TEXT NOT NULL,           -- Lowercase channel name
    sender TEXT NOT NULL,           -- Sender's nickname
    message_data BLOB NOT NULL,     -- JSON envelope (MessageEnvelope)
    nanotime INTEGER NOT NULL,      -- Nanosecond timestamp for precise ordering
    account TEXT                    -- Sender's account name (if logged in)
);

-- Index for channel-based queries (most common)
CREATE INDEX IF NOT EXISTS idx_history_target_time ON message_history(target, nanotime DESC);

-- Index for msgid lookups (for message reference resolution)
CREATE INDEX IF NOT EXISTS idx_history_msgid ON message_history(msgid);
