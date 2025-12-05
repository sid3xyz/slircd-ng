//! UNGROUP command handler for NickServ.

use super::NickServResult;
use crate::db::Database;
use crate::state::Matrix;
use std::sync::Arc;
use tracing::{info, warn};

/// Handle UNGROUP command - unlink a nick from the current account.
pub async fn handle_ungroup(
    db: &Database,
    matrix: &Arc<Matrix>,
    uid: &str,
    args: &[&str],
    reply_effects: impl Fn(&str, Vec<&str>) -> NickServResult,
) -> NickServResult {
    if args.is_empty() {
        return reply_effects(uid, vec!["Syntax: UNGROUP <nick>"]);
    }

    let target_nick = args[0];

    // Must be identified first
    let (account_name, account_id) = if let Some(user) = matrix.users.get(uid) {
        let user = user.read().await;
        if !user.modes.registered {
            return reply_effects(uid, vec!["You must be identified to use this command."]);
        }
        match &user.account {
            Some(name) => {
                // Look up the account ID
                match db.accounts().find_by_name(name).await {
                    Ok(Some(acc)) => (name.clone(), acc.id),
                    _ => return reply_effects(uid, vec!["Account not found."]),
                }
            }
            None => {
                return reply_effects(uid, vec!["You are not identified to any account."]);
            }
        }
    } else {
        return reply_effects(uid, vec!["Internal error."]);
    };

    match db.accounts().unlink_nickname(target_nick, account_id).await {
        Ok(()) => {
            info!(nick = %target_nick, account = %account_name, "Nickname ungrouped");
            reply_effects(
                uid,
                vec![&format!(
                    "Nickname \x02{}\x02 has been removed from your account.",
                    target_nick
                )],
            )
        }
        Err(crate::db::DbError::NicknameNotFound(_)) => reply_effects(
            uid,
            vec![&format!(
                "Nickname \x02{}\x02 is not linked to your account.",
                target_nick
            )],
        ),
        Err(crate::db::DbError::InsufficientAccess) => reply_effects(
            uid,
            vec![&format!(
                "Nickname \x02{}\x02 does not belong to your account.",
                target_nick
            )],
        ),
        Err(crate::db::DbError::UnknownOption(msg)) => reply_effects(uid, vec![&msg]),
        Err(e) => {
            warn!(nick = %target_nick, error = ?e, "UNGROUP failed");
            reply_effects(
                uid,
                vec!["Failed to ungroup nickname. Please try again later."],
            )
        }
    }
}
