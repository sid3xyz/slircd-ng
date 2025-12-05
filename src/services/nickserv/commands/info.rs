//! INFO command handler for NickServ.

use super::NickServResult;
use crate::db::Database;
use tracing::debug;

/// Handle INFO command.
pub async fn handle_info(
    db: &Database,
    uid: &str,
    args: &[&str],
    reply_effect: impl Fn(&str, &str) -> crate::services::ServiceEffect,
    reply_effects: impl Fn(&str, Vec<&str>) -> NickServResult,
) -> NickServResult {
    if args.is_empty() {
        return reply_effects(uid, vec!["Syntax: INFO <nick>"]);
    }

    let nick = args[0];

    match db.accounts().find_by_nickname(nick).await {
        Ok(Some(account)) => {
            let registered_dt = chrono::DateTime::from_timestamp(account.registered_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            let last_seen_dt = chrono::DateTime::from_timestamp(account.last_seen_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            let mut effects = vec![
                reply_effect(uid, &format!("Information on \x02{}\x02:", account.name)),
                reply_effect(uid, &format!("  Registered: {}", registered_dt)),
                reply_effect(uid, &format!("  Last seen:  {}", last_seen_dt)),
            ];

            if !account.hide_email
                && let Some(email) = &account.email
            {
                effects.push(reply_effect(uid, &format!("  Email:      {}", email)));
            }

            if account.enforce {
                effects.push(reply_effect(uid, "  Options:    ENFORCE ON"));
            }

            // Get linked nicknames
            if let Ok(nicks) = db.accounts().get_nicknames(account.id).await
                && !nicks.is_empty()
            {
                effects.push(reply_effect(
                    uid,
                    &format!("  Nicknames:  {}", nicks.join(", ")),
                ));
            }

            effects
        }
        Ok(None) => reply_effects(uid, vec![&format!("\x02{}\x02 is not registered.", nick)]),
        Err(e) => {
            debug!(nick = %nick, error = ?e, "INFO lookup failed");
            reply_effects(uid, vec!["Failed to retrieve account information."])
        }
    }
}
