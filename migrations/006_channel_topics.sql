-- Add topic persistence columns to channels table
-- Topics are saved when KEEPTOPIC is enabled for registered channels

ALTER TABLE channels ADD COLUMN topic_text TEXT;
ALTER TABLE channels ADD COLUMN topic_set_by TEXT;
ALTER TABLE channels ADD COLUMN topic_set_at INTEGER;
