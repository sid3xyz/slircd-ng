//! MLOCK (mode lock) enforcement for registered channels.

use crate::handlers::Context;
use crate::state::RegisteredState;
use slirc_proto::{ChannelMode, Mode};

/// Apply MLOCK filter to mode changes.
/// Returns filtered modes that don't conflict with the channel's MLOCK.
pub(super) async fn apply_mlock_filter(
    ctx: &Context<'_, RegisteredState>,
    channel_lower: &str,
    modes: Vec<Mode<ChannelMode>>,
) -> Vec<Mode<ChannelMode>> {
    // Get channel record from database
    let channel_record = match ctx.db.channels().find_by_name(channel_lower).await {
        Ok(Some(record)) => record,
        _ => return modes, // No MLOCK if not registered or DB error
    };

    let mlock_str = match channel_record.mlock {
        Some(m) if !m.is_empty() => m,
        _ => return modes, // No MLOCK set
    };

    // Parse MLOCK string inline
    let mlock_modes = parse_mlock_inline(&mlock_str);

    // Build sets of locked modes
    let mut locked_on = std::collections::HashSet::with_capacity(mlock_modes.len());
    let mut locked_off = std::collections::HashSet::with_capacity(mlock_modes.len());

    for mlock_mode in mlock_modes {
        let mode_char = mode_to_char(mlock_mode.mode());
        if mlock_mode.is_plus() {
            locked_on.insert(mode_char);
        } else {
            locked_off.insert(mode_char);
        }
    }

    // Filter out conflicting modes
    modes
        .into_iter()
        .filter(|mode| {
            let mode_char = mode_to_char(mode.mode());

            // Skip status modes (they're never MLOCKed)
            if matches!(
                mode.mode(),
                ChannelMode::Oper
                    | ChannelMode::Voice
                    | ChannelMode::Halfop
                    | ChannelMode::Admin
                    | ChannelMode::Founder
            ) {
                return true;
            }

            // Check if mode conflicts with MLOCK
            !((mode.is_plus() && locked_off.contains(&mode_char))
                || (!mode.is_plus() && locked_on.contains(&mode_char)))
        })
        .collect()
}

/// Parse MLOCK string inline (simplified version).
/// Returns list of modes from MLOCK string like "+nt-s" or "+ntk secretkey".
pub(super) fn parse_mlock_inline(mlock: &str) -> Vec<Mode<ChannelMode>> {
    let mut modes = Vec::with_capacity(6); // Typical MLOCK has 3-6 modes
    let trimmed = mlock.trim();
    if trimmed.is_empty() {
        return modes;
    }

    let mut is_plus = true;
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let mode_str = tokens.first().copied().unwrap_or("");

    for ch in mode_str.chars() {
        match ch {
            '+' => is_plus = true,
            '-' => is_plus = false,
            'n' => modes.push(if is_plus {
                Mode::Plus(ChannelMode::NoExternalMessages, None)
            } else {
                Mode::Minus(ChannelMode::NoExternalMessages, None)
            }),
            't' => modes.push(if is_plus {
                Mode::Plus(ChannelMode::ProtectedTopic, None)
            } else {
                Mode::Minus(ChannelMode::ProtectedTopic, None)
            }),
            'i' => modes.push(if is_plus {
                Mode::Plus(ChannelMode::InviteOnly, None)
            } else {
                Mode::Minus(ChannelMode::InviteOnly, None)
            }),
            'm' => modes.push(if is_plus {
                Mode::Plus(ChannelMode::Moderated, None)
            } else {
                Mode::Minus(ChannelMode::Moderated, None)
            }),
            's' => modes.push(if is_plus {
                Mode::Plus(ChannelMode::Secret, None)
            } else {
                Mode::Minus(ChannelMode::Secret, None)
            }),
            'r' => modes.push(if is_plus {
                Mode::Plus(ChannelMode::RegisteredOnly, None)
            } else {
                Mode::Minus(ChannelMode::RegisteredOnly, None)
            }),
            _ => {} // Skip unknown modes
        }
    }

    modes
}

/// Convert ChannelMode to its character representation.
pub(super) fn mode_to_char(mode: &ChannelMode) -> char {
    match mode {
        ChannelMode::NoExternalMessages => 'n',
        ChannelMode::ProtectedTopic => 't',
        ChannelMode::InviteOnly => 'i',
        ChannelMode::Moderated => 'm',
        ChannelMode::Secret => 's',
        ChannelMode::RegisteredOnly => 'r',
        ChannelMode::Key => 'k',
        ChannelMode::Limit => 'l',
        ChannelMode::Ban => 'b',
        ChannelMode::Exception => 'e',
        ChannelMode::InviteException => 'I',
        ChannelMode::Quiet => 'q',
        ChannelMode::NoColors => 'c',
        ChannelMode::NoCTCP => 'C',
        ChannelMode::NoNickChange => 'N',
        ChannelMode::NoKnock => 'K',
        ChannelMode::NoInvite => 'V',
        ChannelMode::NoChannelNotice => 'T',
        ChannelMode::NoKick => 'Q',
        ChannelMode::Permanent => 'P',
        ChannelMode::OperOnly => 'O',
        ChannelMode::FreeInvite => 'g',
        ChannelMode::TlsOnly => 'S',
        ChannelMode::Oper => 'o',
        ChannelMode::Voice => 'v',
        ChannelMode::Halfop => 'h',
        ChannelMode::Admin => 'a',
        ChannelMode::Founder => 'q',
        _ => '?',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::{ChannelMode, Mode};

    #[test]
    fn test_parse_mlock_inline() {
        let modes = parse_mlock_inline("+nt-s");
        assert_eq!(modes.len(), 3);
        // +n
        assert!(matches!(modes[0], Mode::Plus(ChannelMode::NoExternalMessages, None)));
        // +t
        assert!(matches!(modes[1], Mode::Plus(ChannelMode::ProtectedTopic, None)));
        // -s
        assert!(matches!(modes[2], Mode::Minus(ChannelMode::Secret, None)));
    }

    #[test]
    fn test_parse_mlock_empty() {
        let modes = parse_mlock_inline("");
        assert!(modes.is_empty());
    }

    #[test]
    fn test_parse_mlock_spaces() {
        let modes = parse_mlock_inline(" +n ");
        assert_eq!(modes.len(), 1);
        assert!(matches!(modes[0], Mode::Plus(ChannelMode::NoExternalMessages, None)));
    }

    #[test]
    fn test_parse_mlock_unknown() {
        // Unknown chars should be ignored or handled gracefully
        // Implementation:
        /*
            _ => modes.push(if is_plus {
                Mode::Plus(ChannelMode::Unknown(ch), None)
            } else {
                Mode::Minus(ChannelMode::Unknown(ch), None)
            }),
        */
        // Wait, I need to check the implementation of parse_mlock_inline again.
        // It matches specific chars.
        /*
            'n' => ...
            't' => ...
            'i' => ...
            'm' => ...
            's' => ...
            'k' => ...
            'l' => ...
            _ => {} // Ignored
        */
        // So unknown chars are ignored.

        let modes = parse_mlock_inline("+n?t");
        assert_eq!(modes.len(), 2);
        assert!(matches!(modes[0], Mode::Plus(ChannelMode::NoExternalMessages, None)));
        assert!(matches!(modes[1], Mode::Plus(ChannelMode::ProtectedTopic, None)));
    }

    #[test]
    fn test_parse_mlock_switch_polarity() {
        let modes = parse_mlock_inline("+n-t+i");
        assert_eq!(modes.len(), 3);
        assert!(matches!(modes[0], Mode::Plus(ChannelMode::NoExternalMessages, None)));
        assert!(matches!(modes[1], Mode::Minus(ChannelMode::ProtectedTopic, None)));
        assert!(matches!(modes[2], Mode::Plus(ChannelMode::InviteOnly, None)));
    }
}
