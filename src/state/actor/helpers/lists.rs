use super::super::ChannelActor;
use crate::state::ListEntry;
use chrono::Utc;
use slirc_proto::casemap::irc_eq;

impl ChannelActor {
    pub(crate) fn apply_list_mode(
        list: &mut Vec<ListEntry>,
        mask: &str,
        adding: bool,
        set_by: &str,
    ) -> bool {
        // Normalize mask for case-insensitive comparison if needed,
        // but usually masks are case-sensitive or handled by glob matching.
        // However, for exact match removal, we should be consistent.
        // For now, we assume exact string match for removal.

        if adding {
            if list.iter().any(|entry| irc_eq(&entry.mask, mask)) {
                return false;
            }

            list.push(ListEntry {
                mask: mask.to_string(),
                set_by: set_by.to_string(),
                set_at: Utc::now().timestamp(),
            });
            true
        } else {
            let original_len = list.len();
            list.retain(|entry| !irc_eq(&entry.mask, mask));
            original_len != list.len()
        }
    }
}
