-- Channel state persistence
-- Stores runtime channel state across server restarts (separate from ChanServ registration)

CREATE TABLE IF NOT EXISTS channel_state (
    name TEXT PRIMARY KEY NOT NULL,
    modes TEXT NOT NULL DEFAULT '',
    topic TEXT,
    topic_set_by TEXT,
    topic_set_at INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    key TEXT,  -- Channel key (+k mode)
    user_limit INTEGER  -- User limit (+l mode)
);

CREATE INDEX IF NOT EXISTS idx_channel_state_created ON channel_state(created_at);
