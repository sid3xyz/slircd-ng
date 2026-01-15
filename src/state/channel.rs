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

use slirc_proto::sync::clock::HybridTimestamp;

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

#[cfg(test)]
mod tests {
    use super::*;

    // ========== prefix_char tests ==========

    #[test]
    fn prefix_char_none_returns_none() {
        let modes = MemberModes::default();
        assert_eq!(modes.prefix_char(), None);
    }

    #[test]
    fn prefix_char_voice_returns_plus() {
        let modes = MemberModes {
            voice: true,
            ..Default::default()
        };
        assert_eq!(modes.prefix_char(), Some('+'));
    }

    #[test]
    fn prefix_char_halfop_returns_percent() {
        let modes = MemberModes {
            halfop: true,
            ..Default::default()
        };
        assert_eq!(modes.prefix_char(), Some('%'));
    }

    #[test]
    fn prefix_char_op_returns_at() {
        let modes = MemberModes {
            op: true,
            ..Default::default()
        };
        assert_eq!(modes.prefix_char(), Some('@'));
    }

    #[test]
    fn prefix_char_admin_returns_ampersand() {
        let modes = MemberModes {
            admin: true,
            ..Default::default()
        };
        assert_eq!(modes.prefix_char(), Some('&'));
    }

    #[test]
    fn prefix_char_owner_returns_tilde() {
        let modes = MemberModes {
            owner: true,
            ..Default::default()
        };
        assert_eq!(modes.prefix_char(), Some('~'));
    }

    #[test]
    fn prefix_char_priority_owner_over_op() {
        let modes = MemberModes {
            owner: true,
            op: true,
            ..Default::default()
        };
        assert_eq!(modes.prefix_char(), Some('~'));
    }

    #[test]
    fn prefix_char_priority_admin_over_voice() {
        let modes = MemberModes {
            admin: true,
            voice: true,
            ..Default::default()
        };
        assert_eq!(modes.prefix_char(), Some('&'));
    }

    #[test]
    fn prefix_char_priority_op_over_halfop() {
        let modes = MemberModes {
            op: true,
            halfop: true,
            voice: true,
            ..Default::default()
        };
        assert_eq!(modes.prefix_char(), Some('@'));
    }

    // ========== all_prefix_chars tests ==========

    #[test]
    fn all_prefix_chars_none_returns_empty() {
        let modes = MemberModes::default();
        assert_eq!(modes.all_prefix_chars(), "");
    }

    #[test]
    fn all_prefix_chars_voice_only() {
        let modes = MemberModes {
            voice: true,
            ..Default::default()
        };
        assert_eq!(modes.all_prefix_chars(), "+");
    }

    #[test]
    fn all_prefix_chars_op_only() {
        let modes = MemberModes {
            op: true,
            ..Default::default()
        };
        assert_eq!(modes.all_prefix_chars(), "@");
    }

    #[test]
    fn all_prefix_chars_op_and_voice() {
        let modes = MemberModes {
            op: true,
            voice: true,
            ..Default::default()
        };
        assert_eq!(modes.all_prefix_chars(), "@+");
    }

    #[test]
    fn all_prefix_chars_all_modes() {
        let modes = MemberModes {
            owner: true,
            admin: true,
            op: true,
            halfop: true,
            voice: true,
            ..Default::default()
        };
        assert_eq!(modes.all_prefix_chars(), "~&@%+");
    }

    #[test]
    fn all_prefix_chars_admin_and_halfop() {
        let modes = MemberModes {
            admin: true,
            halfop: true,
            ..Default::default()
        };
        assert_eq!(modes.all_prefix_chars(), "&%");
    }

    // ========== has_op_or_higher tests ==========

    #[test]
    fn has_op_or_higher_regular_is_false() {
        let modes = MemberModes::default();
        assert!(!modes.has_op_or_higher());
    }

    #[test]
    fn has_op_or_higher_voice_is_false() {
        let modes = MemberModes {
            voice: true,
            ..Default::default()
        };
        assert!(!modes.has_op_or_higher());
    }

    #[test]
    fn has_op_or_higher_halfop_is_false() {
        let modes = MemberModes {
            halfop: true,
            ..Default::default()
        };
        assert!(!modes.has_op_or_higher());
    }

    #[test]
    fn has_op_or_higher_op_is_true() {
        let modes = MemberModes {
            op: true,
            ..Default::default()
        };
        assert!(modes.has_op_or_higher());
    }

    #[test]
    fn has_op_or_higher_admin_is_true() {
        let modes = MemberModes {
            admin: true,
            ..Default::default()
        };
        assert!(modes.has_op_or_higher());
    }

    #[test]
    fn has_op_or_higher_owner_is_true() {
        let modes = MemberModes {
            owner: true,
            ..Default::default()
        };
        assert!(modes.has_op_or_higher());
    }

    // ========== has_voice_or_higher tests ==========

    #[test]
    fn has_voice_or_higher_regular_is_false() {
        let modes = MemberModes::default();
        assert!(!modes.has_voice_or_higher());
    }

    #[test]
    fn has_voice_or_higher_voice_is_true() {
        let modes = MemberModes {
            voice: true,
            ..Default::default()
        };
        assert!(modes.has_voice_or_higher());
    }

    #[test]
    fn has_voice_or_higher_halfop_is_true() {
        let modes = MemberModes {
            halfop: true,
            ..Default::default()
        };
        assert!(modes.has_voice_or_higher());
    }

    #[test]
    fn has_voice_or_higher_op_is_true() {
        let modes = MemberModes {
            op: true,
            ..Default::default()
        };
        assert!(modes.has_voice_or_higher());
    }

    // ========== has_halfop_or_higher tests ==========

    #[test]
    fn has_halfop_or_higher_regular_is_false() {
        let modes = MemberModes::default();
        assert!(!modes.has_halfop_or_higher());
    }

    #[test]
    fn has_halfop_or_higher_voice_is_false() {
        let modes = MemberModes {
            voice: true,
            ..Default::default()
        };
        assert!(!modes.has_halfop_or_higher());
    }

    #[test]
    fn has_halfop_or_higher_halfop_is_true() {
        let modes = MemberModes {
            halfop: true,
            ..Default::default()
        };
        assert!(modes.has_halfop_or_higher());
    }

    #[test]
    fn has_halfop_or_higher_op_is_true() {
        let modes = MemberModes {
            op: true,
            ..Default::default()
        };
        assert!(modes.has_halfop_or_higher());
    }
}
