//! Channel-related types and state.

use std::collections::HashMap;

/// An IRC channel.
#[derive(Debug)]
pub struct Channel {
    pub name: String,
    pub topic: Option<Topic>,
    pub created: i64,
    /// Members: UID -> MemberModes
    pub members: HashMap<String, MemberModes>,
    /// Channel modes.
    pub modes: ChannelModes,
    /// Ban list (+b).
    pub bans: Vec<ListEntry>,
    /// Ban exception list (+e).
    pub excepts: Vec<ListEntry>,
    /// Invite exception list (+I).
    pub invex: Vec<ListEntry>,
    /// Quiet list (+q).
    pub quiets: Vec<ListEntry>,
    /// Extended ban list (bans with $ prefix like $a:account).
    pub extended_bans: Vec<ListEntry>,
}

/// Channel modes.
#[derive(Debug, Default, Clone)]
pub struct ChannelModes {
    pub invite_only: bool,     // +i
    pub moderated: bool,       // +m
    pub no_external: bool,     // +n
    pub secret: bool,          // +s
    pub topic_lock: bool,      // +t
    pub registered_only: bool, // +r
    pub key: Option<String>,   // +k
    pub limit: Option<u32>,    // +l
    // Advanced channel protection modes
    /// +f - Flood protection: (max_lines, window_seconds)
    /// Kicks users who send more than max_lines in window_seconds
    pub flood_limit: Option<(u32, u32)>,
    /// +L - Channel redirect target when +l limit is reached
    pub redirect: Option<String>,
    /// +j - Join throttle: (max_joins, window_seconds)
    /// Limits joins to max_joins per window_seconds
    pub join_throttle: Option<(u32, u32)>,
    /// +J - Join delay in seconds before user can speak
    pub join_delay: Option<u32>,
    // Extended channel modes
    /// +c - Strip/block color codes
    pub no_colors: bool,
    /// +C - No CTCP (except ACTION)
    pub no_ctcp: bool,
    /// +N - No nick changes while in channel
    pub no_nick_change: bool,
    /// +K - No KNOCK
    pub no_knock: bool,
    /// +V - No INVITE
    pub no_invite: bool,
    /// +T - No channel NOTICE
    pub no_channel_notice: bool,
    /// +u - No kicks (peace mode)
    pub no_kick: bool,
    /// +P - Permanent channel (persists with 0 users)
    pub permanent: bool,
    /// +O - Oper-only channel
    pub oper_only: bool,
    /// +g - Free INVITE (anyone can invite)
    pub free_invite: bool,
    /// +z - TLS-only channel (only TLS clients can join)
    pub tls_only: bool,
}

impl ChannelModes {
    /// Convert modes to a string like "+nt".
    /// Also returns mode parameters in order.
    pub fn as_mode_string(&self) -> String {
        let mut s = String::from("+");
        let mut params = Vec::new();

        if self.invite_only {
            s.push('i');
        }
        if self.moderated {
            s.push('m');
        }
        if self.no_external {
            s.push('n');
        }
        if self.secret {
            s.push('s');
        }
        if self.topic_lock {
            s.push('t');
        }
        if self.registered_only {
            s.push('r');
        }
        if let Some(ref key) = self.key {
            s.push('k');
            params.push(key.clone());
        }
        if let Some(limit) = self.limit {
            s.push('l');
            params.push(limit.to_string());
        }
        if let Some((lines, secs)) = self.flood_limit {
            s.push('f');
            params.push(format!("{}:{}", lines, secs));
        }
        if let Some(ref target) = self.redirect {
            s.push('L');
            params.push(target.clone());
        }
        if let Some((count, secs)) = self.join_throttle {
            s.push('j');
            params.push(format!("{}:{}", count, secs));
        }
        if let Some(delay) = self.join_delay {
            s.push('J');
            params.push(delay.to_string());
        }
        // Extended modes (no parameters)
        if self.no_colors {
            s.push('c');
        }
        if self.no_ctcp {
            s.push('C');
        }
        if self.no_nick_change {
            s.push('N');
        }
        if self.no_knock {
            s.push('K');
        }
        if self.no_invite {
            s.push('V');
        }
        if self.no_channel_notice {
            s.push('T');
        }
        if self.no_kick {
            s.push('u');
        }
        if self.permanent {
            s.push('P');
        }
        if self.oper_only {
            s.push('O');
        }
        if self.free_invite {
            s.push('g');
        }
        if self.tls_only {
            s.push('z');
        }

        if s == "+" {
            "+".to_string()
        } else if params.is_empty() {
            s
        } else {
            format!("{} {}", s, params.join(" "))
        }
    }
}

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

/// Member modes (op, voice, etc.).
#[derive(Debug, Default, Clone)]
pub struct MemberModes {
    pub owner: bool,   // +q (~)
    pub admin: bool,   // +a (&)
    pub op: bool,      // +o (@)
    pub halfop: bool,  // +h (%)
    pub voice: bool,   // +v (+)
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

    /// Get the privilege rank (higher number = more privileges).
    /// Returns 0 if no privileges, 5 for owner, 4 for admin, etc.
    pub fn rank(&self) -> u8 {
        if self.owner {
            5
        } else if self.admin {
            4
        } else if self.op {
            3
        } else if self.halfop {
            2
        } else if self.voice {
            1
        } else {
            0
        }
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

impl Channel {
    /// Create a new channel.
    pub fn new(name: String) -> Self {
        Self {
            name,
            topic: None,
            created: chrono::Utc::now().timestamp(),
            members: HashMap::new(),
            modes: ChannelModes::default(),
            bans: Vec::new(),
            excepts: Vec::new(),
            invex: Vec::new(),
            quiets: Vec::new(),
            extended_bans: Vec::new(),
        }
    }

    /// Add a member to the channel.
    pub fn add_member(&mut self, uid: String, modes: MemberModes) {
        self.members.insert(uid, modes);
    }

    /// Remove a member from the channel.
    pub fn remove_member(&mut self, uid: &str) -> bool {
        self.members.remove(uid).is_some()
    }

    /// Check if user is a member.
    pub fn is_member(&self, uid: &str) -> bool {
        self.members.contains_key(uid)
    }

    /// Check if user has op or higher privileges.
    pub fn is_op(&self, uid: &str) -> bool {
        self.members.get(uid).is_some_and(|m| m.has_op_or_higher())
    }

    /// Check if user has voice or higher (can speak in +m moderated channel).
    pub fn can_speak(&self, uid: &str) -> bool {
        self.members.get(uid).is_some_and(|m| m.has_voice_or_higher())
    }

    /// Check if user has halfop or higher privileges.
    pub fn has_halfop(&self, uid: &str) -> bool {
        self.members.get(uid).is_some_and(|m| m.has_halfop_or_higher())
    }

    /// Get the privilege rank of a user (higher = more privileges).
    pub fn get_rank(&self, uid: &str) -> u8 {
        self.members.get(uid).map(|m| m.rank()).unwrap_or(0)
    }

    /// Check if user_uid can modify target_uid's modes.
    /// A user can only modify users with lower rank, or themselves.
    pub fn can_modify(&self, user_uid: &str, target_uid: &str) -> bool {
        // Users can always modify their own modes
        if user_uid == target_uid {
            return true;
        }
        let user_rank = self.get_rank(user_uid);
        let target_rank = self.get_rank(target_uid);
        user_rank > target_rank
    }

    /// Get list of member UIDs.
    #[allow(dead_code)] // TODO: Use for WHO #channel and NAMES
    pub fn member_uids(&self) -> Vec<String> {
        self.members.keys().cloned().collect()
    }
}
