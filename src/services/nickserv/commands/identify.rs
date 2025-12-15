//! IDENTIFY command handler for NickServ.

use super::NickServResult;
use crate::db::Database;
use crate::services::ServiceEffect;
use tracing::{info, warn};

/// Handle IDENTIFY command.
pub async fn handle_identify(
    db: &Database,
    uid: &str,
    nick: &str,
    args: &[&str],
    reply_effect: impl Fn(&str, &str) -> ServiceEffect,
    reply_effects: impl Fn(&str, Vec<&str>) -> NickServResult,
) -> NickServResult {
    if args.is_empty() {
        return reply_effects(uid, vec!["Syntax: IDENTIFY <password>"]);
    }

    let password = args[0];

    match db.accounts().identify(nick, password).await {
        Ok(account) => {
            info!(nick = %nick, account = %account.name, "User identified");
            vec![
                reply_effect(
                    uid,
                    &format!("You are now identified for \x02{}\x02.", account.name),
                ),
                ServiceEffect::AccountIdentify {
                    target_uid: uid.to_string(),
                    account: account.name.clone(),
                },
                ServiceEffect::BroadcastAccount {
                    target_uid: uid.to_string(),
                    new_account: account.name,
                },
                // Cancel any pending nick enforcement timer
                ServiceEffect::ClearEnforceTimer {
                    target_uid: uid.to_string(),
                },
            ]
        }
        Err(crate::db::DbError::AccountNotFound(_)) => {
            reply_effects(uid, vec!["No account found for your nickname."])
        }
        Err(crate::db::DbError::InvalidPassword) => reply_effects(uid, vec!["Invalid password."]),
        Err(e) => {
            warn!(nick = %nick, error = ?e, "Identification failed");
            reply_effects(uid, vec!["Identification failed. Please try again later."])
        }
    }
}
