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

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_crdt::clock::{HybridTimestamp, ServerId};

    /// Helper to create a test timestamp
    fn test_ts() -> HybridTimestamp {
        HybridTimestamp::new(0, 0, &ServerId::new("test"))
    }

    #[test]
    fn test_modes_to_string_empty_set() {
        let modes = HashSet::new();
        let result = modes_to_string(&modes);
        // Empty set should just have the + prefix
        assert_eq!(result, "+");
    }

    #[test]
    fn test_modes_to_string_single_mode() {
        let mut modes = HashSet::new();
        modes.insert(ChannelMode::NoExternal);
        let result = modes_to_string(&modes);
        assert_eq!(result, "+n");
    }

    #[test]
    fn test_modes_to_string_multiple_simple_modes() {
        let mut modes = HashSet::new();
        modes.insert(ChannelMode::NoExternal);
        modes.insert(ChannelMode::TopicLock);
        modes.insert(ChannelMode::Secret);

        let result = modes_to_string(&modes);

        // Should start with +
        assert!(result.starts_with('+'));
        // Should contain all three mode chars (order may vary)
        assert!(result.contains('n'));
        assert!(result.contains('t'));
        assert!(result.contains('s'));
        // Should be exactly 4 chars: +, n, t, s
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_modes_to_string_with_key() {
        let mut modes = HashSet::new();
        modes.insert(ChannelMode::NoExternal);
        modes.insert(ChannelMode::Key("secret".to_string(), test_ts()));

        let result = modes_to_string(&modes);

        // Should contain the key mode and the key value
        assert!(result.contains('k'));
        assert!(result.contains("secret"));
        assert!(result.contains(' ')); // space before param
    }

    #[test]
    fn test_modes_to_string_with_limit() {
        let mut modes = HashSet::new();
        modes.insert(ChannelMode::Limit(50, test_ts()));

        let result = modes_to_string(&modes);

        assert!(result.contains('l'));
        assert!(result.contains("50"));
    }

    #[test]
    fn test_modes_to_string_with_key_and_limit() {
        let mut modes = HashSet::new();
        modes.insert(ChannelMode::Key("pass".to_string(), test_ts()));
        modes.insert(ChannelMode::Limit(100, test_ts()));
        modes.insert(ChannelMode::InviteOnly);

        let result = modes_to_string(&modes);

        // Should have i, k, l modes
        assert!(result.contains('i'));
        assert!(result.contains('k'));
        assert!(result.contains('l'));
        // Should have both param values
        assert!(result.contains("pass"));
        assert!(result.contains("100"));
    }

    #[test]
    fn test_modes_to_string_all_simple_modes() {
        let mut modes = HashSet::new();
        modes.insert(ChannelMode::NoExternal);
        modes.insert(ChannelMode::TopicLock);
        modes.insert(ChannelMode::Moderated);
        modes.insert(ChannelMode::Secret);
        modes.insert(ChannelMode::Private);
        modes.insert(ChannelMode::InviteOnly);
        modes.insert(ChannelMode::NoCtcp);
        modes.insert(ChannelMode::Permanent);
        modes.insert(ChannelMode::RegisteredOnly);

        let result = modes_to_string(&modes);

        // Check all expected mode chars are present
        for c in ['n', 't', 'm', 's', 'p', 'i', 'C', 'P', 'R'] {
            assert!(result.contains(c), "Missing mode char: {}", c);
        }
    }

    // Tests for mode_to_char (via ChannelActor implementation)
    // Note: mode_to_char is a method on ChannelActor, so we test it indirectly
    // by verifying the mode character mappings match what modes_to_string uses.

    #[test]
    fn test_mode_to_char_basic_modes() {
        // Verify mode_to_char returns expected characters
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::NoExternal),
            Some('n')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::TopicLock),
            Some('t')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::Moderated),
            Some('m')
        );
        assert_eq!(ChannelActor::mode_to_char(&ChannelMode::Secret), Some('s'));
        assert_eq!(ChannelActor::mode_to_char(&ChannelMode::Private), Some('p'));
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::InviteOnly),
            Some('i')
        );
    }

    #[test]
    fn test_mode_to_char_extended_modes() {
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::ModeratedUnreg),
            Some('M')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::OpModerated),
            Some('U')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::NoNickChange),
            Some('N')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::NoColors),
            Some('c')
        );
        assert_eq!(ChannelActor::mode_to_char(&ChannelMode::TlsOnly), Some('z'));
        assert_eq!(ChannelActor::mode_to_char(&ChannelMode::NoKnock), Some('K'));
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::NoInvite),
            Some('V')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::NoNotice),
            Some('T')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::FreeInvite),
            Some('g')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::OperOnly),
            Some('O')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::AdminOnly),
            Some('A')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::Auditorium),
            Some('u')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::Registered),
            Some('r')
        );
        assert_eq!(ChannelActor::mode_to_char(&ChannelMode::NoKicks), Some('Q'));
        assert_eq!(ChannelActor::mode_to_char(&ChannelMode::NoCtcp), Some('C'));
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::Permanent),
            Some('P')
        );
        assert_eq!(
            ChannelActor::mode_to_char(&ChannelMode::RegisteredOnly),
            Some('R')
        );
    }

    #[test]
    fn test_mode_to_char_param_modes_return_none() {
        // Parametric modes should return None (they use separate timestamp fields)
        let key_mode = ChannelMode::Key("test".to_string(), test_ts());
        let limit_mode = ChannelMode::Limit(100, test_ts());

        assert_eq!(ChannelActor::mode_to_char(&key_mode), None);
        assert_eq!(ChannelActor::mode_to_char(&limit_mode), None);
    }
}
