use super::super::ChannelActor;
use crate::state::ListEntry;
use chrono::Utc;
use slirc_proto::casemap::{irc_eq, irc_to_lower};

/// Maximum length of a ban/exception mask (nick!user@host pattern).
/// Most IRC servers use 250-500 bytes. We use 350 to allow generous masks.
const MAX_MASK_LENGTH: usize = 350;

/// Maximum entries per list mode (bans, excepts, invex, quiets).
/// Prevents memory exhaustion from excessive ban lists.
const MAX_LIST_ENTRIES: usize = 100;

impl ChannelActor {
    pub(crate) fn apply_list_mode(
        list: &mut Vec<ListEntry>,
        mask: &str,
        adding: bool,
        set_by: &str,
    ) -> bool {
        // Validate mask length
        if mask.len() > MAX_MASK_LENGTH {
            return false;
        }

        // CRITICAL FIX: Normalize mask to lowercase using RFC 1459 case mapping
        // for case-insensitive add/remove (irctest chmodes/ban.py).
        // Example: +b BAR!*@* should be removable with -b bar!*@*
        let normalized_mask = irc_to_lower(mask);

        if adding {
            // Check list size limit
            if list.len() >= MAX_LIST_ENTRIES {
                return false;
            }

            if list.iter().any(|entry| irc_eq(&entry.mask, &normalized_mask)) {
                return false;
            }

            list.push(ListEntry {
                mask: normalized_mask,  // Store normalized form for consistent lookups
                set_by: set_by.to_string(),
                set_at: Utc::now().timestamp(),
            });
            true
        } else {
            let original_len = list.len();
            list.retain(|entry| !irc_eq(&entry.mask, &normalized_mask));
            original_len != list.len()
        }
    }
}
