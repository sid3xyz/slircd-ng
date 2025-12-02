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
}

impl ChannelModes {
    /// Convert modes to a string like "+nt".
    pub fn as_mode_string(&self) -> String {
        let mut s = String::from("+");
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
        if self.key.is_some() {
            s.push('k');
        }
        if self.limit.is_some() {
            s.push('l');
        }
        if s == "+" { "+".to_string() } else { s }
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
    pub op: bool,    // +o
    pub voice: bool, // +v
}

impl MemberModes {
    /// Get the highest prefix character for this member.
    pub fn prefix_char(&self) -> Option<char> {
        if self.op {
            Some('@')
        } else if self.voice {
            Some('+')
        } else {
            None
        }
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

    /// Check if user has op.
    pub fn is_op(&self, uid: &str) -> bool {
        self.members.get(uid).is_some_and(|m| m.op)
    }

    /// Check if user has voice or higher.
    #[allow(dead_code)] // TODO: Use for +m moderated channel enforcement
    pub fn can_speak(&self, uid: &str) -> bool {
        self.members.get(uid).is_some_and(|m| m.op || m.voice)
    }

    /// Get list of member UIDs.
    #[allow(dead_code)] // TODO: Use for WHO #channel and NAMES
    pub fn member_uids(&self) -> Vec<String> {
        self.members.keys().cloned().collect()
    }
}
