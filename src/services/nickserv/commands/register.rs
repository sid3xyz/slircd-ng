//! REGISTER command handler for NickServ.

use super::NickServResult;
use crate::db::Database;
use crate::services::ServiceEffect;
use tracing::{info, warn};

/// Handle REGISTER command.
pub async fn handle_register(
    db: &Database,
    uid: &str,
    nick: &str,
    args: &[&str],
    reply_effect: impl Fn(&str, &str) -> ServiceEffect,
    reply_effects: impl Fn(&str, Vec<&str>) -> NickServResult,
) -> NickServResult {
    if args.is_empty() {
        return reply_effects(uid, vec!["Syntax: REGISTER <password> [email]"]);
    }

    let password = args[0];
    let email = args.get(1).copied();

    match db.accounts().register(nick, password, email).await {
        Ok(account) => {
            info!(nick = %nick, account = %account.name, "Account registered");
            vec![
                reply_effect(
                    uid,
                    &format!("Your nickname \x02{}\x02 has been registered.", nick),
                ),
                reply_effect(uid, "You are now identified to your account."),
                ServiceEffect::AccountIdentify {
                    target_uid: uid.to_string(),
                    account: account.name,
                },
            ]
        }
        Err(crate::db::DbError::AccountExists(name)) => reply_effects(
            uid,
            vec![&format!(
                "An account named \x02{}\x02 already exists.",
                name
            )],
        ),
        Err(crate::db::DbError::NicknameRegistered(name)) => reply_effects(
            uid,
            vec![&format!(
                "The nickname \x02{}\x02 is already registered.",
                name
            )],
        ),
        Err(e) => {
            warn!(nick = %nick, error = ?e, "Registration failed");
            reply_effects(uid, vec!["Registration failed. Please try again later."])
        }
    }
}
