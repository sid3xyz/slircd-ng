//! GROUP command handler for NickServ.

use super::NickServResult;
use crate::db::Database;
use crate::services::ServiceEffect;
use tracing::{info, warn};

/// Handle GROUP command - link current nick to an existing account.
pub async fn handle_group(
    db: &Database,
    uid: &str,
    nick: &str,
    args: &[&str],
    reply_effect: impl Fn(&str, &str) -> ServiceEffect,
    reply_effects: impl Fn(&str, Vec<&str>) -> NickServResult,
) -> NickServResult {
    if args.len() < 2 {
        return reply_effects(uid, vec!["Syntax: GROUP <account> <password>"]);
    }

    let account_name = args[0];
    let password = args[1];

    match db
        .accounts()
        .link_nickname(nick, account_name, password)
        .await
    {
        Ok(()) => {
            info!(nick = %nick, account = %account_name, "Nickname grouped");
            vec![
                reply_effect(
                    uid,
                    &format!(
                        "Your nickname \x02{}\x02 is now linked to account \x02{}\x02.",
                        nick, account_name
                    ),
                ),
                reply_effect(uid, "You are now identified to your account."),
                ServiceEffect::AccountIdentify {
                    target_uid: uid.to_string(),
                    account: account_name.to_string(),
                },
            ]
        }
        Err(crate::db::DbError::AccountNotFound(_)) => reply_effects(
            uid,
            vec![&format!("Account \x02{}\x02 does not exist.", account_name)],
        ),
        Err(crate::db::DbError::InvalidPassword) => reply_effects(uid, vec!["Invalid password."]),
        Err(crate::db::DbError::NicknameRegistered(_)) => reply_effects(
            uid,
            vec![&format!(
                "Nickname \x02{}\x02 is already registered to another account.",
                nick
            )],
        ),
        Err(e) => {
            warn!(nick = %nick, account = %account_name, error = ?e, "GROUP failed");
            reply_effects(
                uid,
                vec!["Failed to group nickname. Please try again later."],
            )
        }
    }
}
