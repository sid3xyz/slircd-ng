use super::super::{ChannelActor, Uid};
use crate::state::ListEntry;
use chrono::Utc;

impl ChannelActor {
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
}
