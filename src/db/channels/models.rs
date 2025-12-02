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
}

/// Channel access entry.
#[derive(Debug, Clone)]
pub struct ChannelAccess {
    #[allow(dead_code)] // Stored for potential future use in access queries
    pub channel_id: i64,
    pub account_id: i64,
    pub flags: String,
    pub added_by: String,
    pub added_at: i64,
}

/// A channel AKICK entry.
#[derive(Debug, Clone)]
#[allow(dead_code)] // TODO: Use for AKICK LIST command
pub struct ChannelAkick {
    pub id: i64,
    pub channel_id: i64,
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
}
