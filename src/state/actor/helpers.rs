use super::{ChannelActor, ChannelMode, Uid};
use crate::state::{ListEntry, MemberModes};
use chrono::Utc;
use std::collections::HashSet;

impl ChannelActor {
    pub(crate) fn set_flag_mode(&mut self, flag: ChannelMode, adding: bool) -> bool {
        if adding {
            self.modes.insert(flag)
        } else {
            self.modes.remove(&flag)
        }
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

    pub(crate) fn apply_list_mode(
        list: &mut Vec<ListEntry>,
        mask: &str,
        adding: bool,
        set_by: &Uid,
    ) -> bool {
        if adding {
            if list.iter().any(|entry| entry.mask == mask) {
                return false;
            }

            list.push(ListEntry {
                mask: mask.to_string(),
                set_by: set_by.clone(),
                set_at: Utc::now().timestamp(),
            });
            true
        } else {
            let original_len = list.len();
            list.retain(|entry| entry.mask != mask);
            original_len != list.len()
        }
    }

    pub(crate) fn update_member_mode<F>(&mut self, target_uid: &Uid, mut update: F) -> bool
    where
        F: FnMut(&mut MemberModes),
    {
        if let Some(member) = self.members.get(target_uid).cloned() {
            let mut updated = member.clone();
            update(&mut updated);

            if updated != member {
                self.members.insert(target_uid.clone(), updated);
                return true;
            }
        }

        false
    }
}

/// Convert channel modes to a string representation (e.g. "+ntk key").
pub fn modes_to_string(modes: &HashSet<ChannelMode>) -> String {
    let mut flags = String::new();
    let mut params = Vec::new();

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
    if modes.contains(&ChannelMode::SSLOnly) {
        flags.push('S');
    }

    // Param modes
    for mode in modes {
        match mode {
            ChannelMode::Key(k) => {
                if !flags.contains('k') {
                    flags.push('k');
                    params.push(k.clone());
                }
            }
            ChannelMode::Limit(l) => {
                if !flags.contains('l') {
                    flags.push('l');
                    params.push(l.to_string());
                }
            }
            ChannelMode::Redirect(c) => {
                if !flags.contains('L') {
                    flags.push('L');
                    params.push(c.clone());
                }
            }
            ChannelMode::JoinDelay(s) => {
                if !flags.contains('J') {
                    flags.push('J');
                    params.push(s.to_string());
                }
            }
            ChannelMode::JoinThrottle { joins, seconds } => {
                if !flags.contains('j') {
                    flags.push('j');
                    params.push(format!("{}:{}", joins, seconds));
                }
            }
            ChannelMode::FloodProtection { messages, seconds } => {
                if !flags.contains('f') {
                    flags.push('f');
                    params.push(format!("{}:{}", messages, seconds));
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
