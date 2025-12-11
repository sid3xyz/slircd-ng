use super::super::ChannelActor;
use crate::state::ListEntry;
use chrono::Utc;
use slirc_proto::casemap::{irc_eq, irc_to_lower};

impl ChannelActor {
    pub(crate) fn apply_list_mode(
        list: &mut Vec<ListEntry>,
        mask: &str,
        adding: bool,
        set_by: &str,
    ) -> bool {
        // CRITICAL FIX: Normalize mask to lowercase using RFC 1459 case mapping
        // for case-insensitive add/remove (irctest chmodes/ban.py).
        // Example: +b BAR!*@* should be removable with -b bar!*@*
        let normalized_mask = irc_to_lower(mask);

        if adding {
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
