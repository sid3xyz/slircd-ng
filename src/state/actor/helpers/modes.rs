//! Channel mode utilities.
//!
//! Helpers for setting/clearing channel modes and converting to strings.

use super::super::{ChannelActor, ChannelMode};
use slirc_crdt::clock::HybridTimestamp;
use std::collections::HashSet;

impl ChannelActor {
    /// Get the mode character for a `ChannelMode` variant for timestamp tracking.
    pub(crate) fn mode_to_char(mode: &ChannelMode) -> Option<char> {
        match mode {
            ChannelMode::NoExternal => Some('n'),
            ChannelMode::TopicLock => Some('t'),
            ChannelMode::Moderated => Some('m'),
            ChannelMode::ModeratedUnreg => Some('M'),
            ChannelMode::OpModerated => Some('U'),
            ChannelMode::NoNickChange => Some('N'),
            ChannelMode::NoColors => Some('c'),
            ChannelMode::TlsOnly => Some('z'),
            ChannelMode::NoKnock => Some('K'),
            ChannelMode::NoInvite => Some('V'),
            ChannelMode::NoNotice => Some('T'),
            ChannelMode::FreeInvite => Some('g'),
            ChannelMode::OperOnly => Some('O'),
            ChannelMode::AdminOnly => Some('A'),
            ChannelMode::Auditorium => Some('u'),
            ChannelMode::Registered => Some('r'),
            ChannelMode::NoKicks => Some('Q'),
            ChannelMode::Secret => Some('s'),
            ChannelMode::Private => Some('p'),
            ChannelMode::InviteOnly => Some('i'),
            ChannelMode::NoCtcp => Some('C'),
            ChannelMode::Permanent => Some('P'),
            ChannelMode::RegisteredOnly => Some('R'),
            ChannelMode::Key(_, _) | ChannelMode::Limit(_, _) => None, // Parametric modes use separate timestamp fields
        }
    }

    #[allow(clippy::collapsible_if)]
    pub(crate) fn set_flag_mode(&mut self, flag: ChannelMode, adding: bool) -> bool {
        let changed = if adding {
            self.modes.insert(flag.clone())
        } else {
            self.modes.remove(&flag)
        };

        // Record timestamp for boolean mode changes
        if changed {
            if let Some(mode_char) = Self::mode_to_char(&flag) {
                self.mode_timestamps
                    .insert(mode_char, HybridTimestamp::now(&self.server_id));
            }
        }

        changed
    }

    pub(crate) fn replace_param_mode<F>(
        &mut self,
        predicate: F,
        new_mode: Option<ChannelMode>,
    ) -> bool
    where
        F: Fn(&ChannelMode) -> bool,
    {
        let mut changed = false;
        self.modes.retain(|mode| {
            let remove = predicate(mode);
            if remove {
                changed = true;
            }
            !remove
        });

        if let Some(mode) = new_mode {
            changed |= self.modes.insert(mode);
        }

        changed
    }
}

/// Convert channel modes to a string representation (e.g. "+ntk key").
pub fn modes_to_string(modes: &HashSet<ChannelMode>) -> String {
    let mut flags = String::new();
    let mut params = Vec::with_capacity(3); // key, limit, forward

    flags.push('+');

    // Simple modes
    if modes.contains(&ChannelMode::NoExternal) {
        flags.push('n');
    }
    if modes.contains(&ChannelMode::TopicLock) {
        flags.push('t');
    }
    if modes.contains(&ChannelMode::Moderated) {
        flags.push('m');
    }
    if modes.contains(&ChannelMode::ModeratedUnreg) {
        flags.push('M');
    }
    if modes.contains(&ChannelMode::OpModerated) {
        flags.push('U');
    }
    if modes.contains(&ChannelMode::NoNickChange) {
        flags.push('N');
    }
    if modes.contains(&ChannelMode::NoColors) {
        flags.push('c');
    }
    if modes.contains(&ChannelMode::TlsOnly) {
        flags.push('z');
    }
    if modes.contains(&ChannelMode::NoKnock) {
        flags.push('K');
    }
    if modes.contains(&ChannelMode::NoInvite) {
        flags.push('V');
    }
    if modes.contains(&ChannelMode::NoNotice) {
        flags.push('T');
    }
    if modes.contains(&ChannelMode::FreeInvite) {
        flags.push('g');
    }
    if modes.contains(&ChannelMode::OperOnly) {
        flags.push('O');
    }
    if modes.contains(&ChannelMode::AdminOnly) {
        flags.push('A');
    }
    if modes.contains(&ChannelMode::Auditorium) {
        flags.push('u');
    }
    if modes.contains(&ChannelMode::Registered) {
        flags.push('r');
    }
    if modes.contains(&ChannelMode::NoKicks) {
        flags.push('Q');
    }
    if modes.contains(&ChannelMode::Secret) {
        flags.push('s');
    }
    if modes.contains(&ChannelMode::Private) {
        flags.push('p');
    }
    if modes.contains(&ChannelMode::InviteOnly) {
        flags.push('i');
    }
    if modes.contains(&ChannelMode::NoCtcp) {
        flags.push('C');
    }
    if modes.contains(&ChannelMode::Permanent) {
        flags.push('P');
    }
    if modes.contains(&ChannelMode::RegisteredOnly) {
        flags.push('R');
    }

    // Param modes
    for mode in modes {
        match mode {
            ChannelMode::Key(k, _) => {
                if !flags.contains('k') {
                    flags.push('k');
                    params.push(k.clone());
                }
            }
            ChannelMode::Limit(l, _) => {
                if !flags.contains('l') {
                    flags.push('l');
                    params.push(l.to_string());
                }
            }
            _ => {}
        }
    }

    if params.is_empty() {
        flags
    } else {
        format!("{} {}", flags, params.join(" "))
    }
}
