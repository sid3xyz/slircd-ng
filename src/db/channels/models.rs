//! Channel database models.

/// A registered ChanServ channel.
#[derive(Debug, Clone)]
pub struct ChannelRecord {
    pub id: i64,
    pub name: String,
    pub founder_account_id: i64,
    pub registered_at: i64,
    pub last_used_at: i64,
    pub description: Option<String>,
    pub mlock: Option<String>,
    pub keeptopic: bool,
    /// Persisted topic text (when keeptopic is enabled)
    pub topic_text: Option<String>,
    /// Who set the persisted topic
    pub topic_set_by: Option<String>,
    /// When the persisted topic was set (Unix timestamp)
    pub topic_set_at: Option<i64>,
}

/// Channel access entry.
#[derive(Debug, Clone)]
pub struct ChannelAccess {
    pub account_id: i64,
    pub flags: String,
    pub added_by: String,
    pub added_at: i64,
}

/// A channel AKICK entry.
#[derive(Debug, Clone)]
pub struct ChannelAkick {
    #[allow(dead_code)]
    pub id: i64,
    #[allow(dead_code)]
    pub channel_id: i64,
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
}
