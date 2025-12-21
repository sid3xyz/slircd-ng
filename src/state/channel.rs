//! Channel-related types and state.

/// An entry in a list (bans, excepts, invex).
#[derive(Debug, Clone)]
pub struct ListEntry {
    pub mask: String,
    pub set_by: String,
    pub set_at: i64,
}

/// Channel topic with metadata.
#[derive(Debug, Clone)]
pub struct Topic {
    pub text: String,
    pub set_by: String,
    pub set_at: i64,
}

use slirc_crdt::clock::HybridTimestamp;

/// Member modes (op, voice, etc.).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MemberModes {
    pub owner: bool, // +q (~)
    pub owner_ts: Option<HybridTimestamp>,
    pub admin: bool, // +a (&)
    pub admin_ts: Option<HybridTimestamp>,
    pub op: bool, // +o (@)
    pub op_ts: Option<HybridTimestamp>,
    pub halfop: bool, // +h (%)
    pub halfop_ts: Option<HybridTimestamp>,
    pub voice: bool, // +v (+)
    pub voice_ts: Option<HybridTimestamp>,
    /// Timestamp when user joined the channel (for +J enforcement)
    pub join_time: Option<i64>,
}

impl MemberModes {
    /// Get the highest prefix character for this member.
    /// Priority: ~ > & > @ > % > +
    pub fn prefix_char(&self) -> Option<char> {
        if self.owner {
            Some('~')
        } else if self.admin {
            Some('&')
        } else if self.op {
            Some('@')
        } else if self.halfop {
            Some('%')
        } else if self.voice {
            Some('+')
        } else {
            None
        }
    }

    /// Get all prefix characters for this member (for multi-prefix CAP).
    /// Returns in order from highest to lowest: ~ & @ % +
    pub fn all_prefix_chars(&self) -> String {
        // Max 5 prefix chars: ~ & @ % +
        let mut s = String::with_capacity(5);
        if self.owner {
            s.push('~');
        }
        if self.admin {
            s.push('&');
        }
        if self.op {
            s.push('@');
        }
        if self.halfop {
            s.push('%');
        }
        if self.voice {
            s.push('+');
        }
        s
    }

    /// Check if this member has operator privileges (op or higher).
    pub fn has_op_or_higher(&self) -> bool {
        self.owner || self.admin || self.op
    }

    /// Check if this member has voice or higher (can speak in moderated channel).
    pub fn has_voice_or_higher(&self) -> bool {
        self.owner || self.admin || self.op || self.halfop || self.voice
    }

    /// Check if this member has halfop or higher (can kick, change some modes).
    pub fn has_halfop_or_higher(&self) -> bool {
        self.owner || self.admin || self.op || self.halfop
    }
}
