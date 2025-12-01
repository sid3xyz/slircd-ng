//! DROP command handler for NickServ.

use crate::db::Database;
use crate::services::ServiceEffect;
use super::NickServResult;
use tracing::{info, warn};

/// Handle DROP command.
pub async fn handle_drop(
    db: &Database,
    uid: &str,
    nick: &str,
    args: &[&str],
    reply_effect: impl Fn(&str, &str) -> ServiceEffect,
    reply_effects: impl Fn(&str, Vec<&str>) -> NickServResult,
) -> NickServResult {
    if args.is_empty() {
        return reply_effects(uid, vec!["Syntax: DROP <password>"]);
    }

    let password = args[0];

    // Verify the user owns the account for their current nick
    match db.accounts().drop_account(nick, password).await {
        Ok(()) => {
            info!(nick = %nick, "Account dropped");
            vec![
                reply_effect(
                    uid,
                    &format!("Your account \x02{}\x02 has been dropped.", nick),
                ),
                reply_effect(uid, "All associated nicknames have been released."),
                ServiceEffect::AccountClear {
                    target_uid: uid.to_string(),
                },
            ]
        }
        Err(crate::db::DbError::AccountNotFound(_)) => {
            reply_effects(uid, vec!["Your nickname is not registered."])
        }
        Err(crate::db::DbError::InvalidPassword) => {
            reply_effects(uid, vec!["Invalid password."])
        }
        Err(e) => {
            warn!(nick = %nick, error = ?e, "DROP failed");
            reply_effects(uid, vec!["Failed to drop account. Please try again later."])
        }
    }
}
