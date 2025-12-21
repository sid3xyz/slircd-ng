//! GHOST command handler for NickServ.

use super::NickServResult;
use crate::db::Database;
use crate::services::ServiceEffect;
use crate::state::Matrix;
use std::sync::Arc;
use tracing::info;

/// Handle GHOST command.
pub async fn handle_ghost(
    db: &Database,
    matrix: &Arc<Matrix>,
    uid: &str,
    nick: &str,
    args: &[&str],
    reply_effect: impl Fn(&str, &str) -> ServiceEffect,
    reply_effects: impl Fn(&str, Vec<&str>) -> NickServResult,
) -> NickServResult {
    if args.is_empty() {
        return reply_effects(uid, vec!["Syntax: GHOST <nick> [password]"]);
    }

    let target_nick = args[0];
    let password = args.get(1).copied();

    // Check if the user is already identified and get their account
    let user_arc = matrix.user_manager.users.get(uid).map(|u| u.clone());
    let user_account = if let Some(user_arc) = user_arc {
        let user = user_arc.read().await;
        if user.modes.registered {
            user.account.clone()
        } else {
            None
        }
    } else {
        None
    };

    // Verify authorization
    let authorized = if let Some(ref account_name) = user_account {
        // User is identified, check if target nick belongs to their account
        if let Some(target_account) = db
            .accounts()
            .find_by_nickname(target_nick)
            .await
            .ok()
            .flatten()
        {
            // Check if target belongs to the same account
            target_account.name.eq_ignore_ascii_case(account_name)
        } else {
            false
        }
    } else if let Some(pw) = password {
        // Try to identify with password
        db.accounts().identify(target_nick, pw).await.is_ok()
    } else {
        false
    };

    if !authorized {
        return reply_effects(
            uid,
            vec!["Access denied. You must be identified or provide the correct password."],
        );
    }

    // Find the target user
    let target_nick_lower = slirc_proto::irc_to_lower(target_nick);
    if let Some(target_uid) = matrix
        .user_manager
        .nicks
        .get(&target_nick_lower)
        .map(|r| r.clone())
    {
        if target_uid == uid {
            return reply_effects(uid, vec!["You cannot ghost yourself."]);
        }

        info!(nick = %nick, target = %target_nick, "Ghost requested");
        vec![
            reply_effect(uid, &format!("\x02{}\x02 has been ghosted.", target_nick)),
            ServiceEffect::Kill {
                target_uid,
                killer: "NickServ".to_string(),
                reason: format!("Ghosted by {}", nick),
            },
        ]
    } else {
        reply_effects(
            uid,
            vec![&format!("\x02{}\x02 is not online.", target_nick)],
        )
    }
}
