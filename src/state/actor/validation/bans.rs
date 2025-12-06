use crate::security::{UserContext, matches_ban_or_except};
use crate::state::ListEntry;

/// Create IRC user mask (nick!user@host).
pub fn format_user_mask(nick: &str, user: &str, host: &str) -> String {
    format!("{}!{}@{}", nick, user, host)
}

/// Create IRC user mask from a UserContext.
pub fn create_user_mask(user_context: &UserContext) -> String {
    format_user_mask(
        &user_context.nickname,
        &user_context.username,
        &user_context.hostname,
    )
}

/// Check if a user is banned, accounting for exceptions.
pub fn is_banned(
    user_mask: &str,
    user_context: &UserContext,
    bans: &[ListEntry],
    excepts: &[ListEntry],
) -> bool {
    for ban in bans {
        if matches_ban_or_except(&ban.mask, user_mask, user_context) {
            let is_excepted = excepts
                .iter()
                .any(|e| matches_ban_or_except(&e.mask, user_mask, user_context));

            if !is_excepted {
                return true;
            }
        }
    }

    false
}
