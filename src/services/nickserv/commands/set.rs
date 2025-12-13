//! SET command handler for NickServ.

use super::NickServResult;
use crate::db::Database;
use crate::state::Matrix;
use std::sync::Arc;
use tracing::{info, warn};

/// Handle SET command.
pub async fn handle_set(
    db: &Database,
    matrix: &Arc<Matrix>,
    uid: &str,
    args: &[&str],
    reply_effect: impl Fn(&str, &str) -> crate::services::ServiceEffect,
    reply_effects: impl Fn(&str, Vec<&str>) -> NickServResult,
) -> NickServResult {
    if args.len() < 2 {
        return vec![
            reply_effect(uid, "Syntax: SET <option> <value>"),
            reply_effect(uid, "Options:"),
            reply_effect(uid, "  EMAIL <address> - Set email address"),
            reply_effect(
                uid,
                "  ENFORCE ON|OFF  - Enable/disable nickname enforcement",
            ),
            reply_effect(uid, "  HIDEMAIL ON|OFF - Hide/show email in INFO"),
            reply_effect(uid, "  PASSWORD <pass> - Change password"),
        ];
    }

    // Check if user is identified and get their account name
    let user_arc = matrix.users.get(uid).map(|u| u.clone());
    let account_name = if let Some(user_arc) = user_arc {
        let user = user_arc.read().await;
        if !user.modes.registered {
            return reply_effects(uid, vec!["You are not identified to any account."]);
        }
        match &user.account {
            Some(name) => name.clone(),
            None => {
                return reply_effects(uid, vec!["You are not identified to any account."]);
            }
        }
    } else {
        return reply_effects(uid, vec!["Internal error."]);
    };

    // Find account
    let account = match db.accounts().find_by_name(&account_name).await {
        Ok(Some(acc)) => acc,
        _ => return reply_effects(uid, vec!["Account not found."]),
    };

    let option = args[0];
    let value = args[1];

    match db.accounts().set_option(account.id, option, value).await {
        Ok(()) => {
            info!(account = %account.name, option = %option, "Account setting changed");
            reply_effects(
                uid,
                vec![&format!(
                    "\x02{}\x02 has been set to \x02{}\x02.",
                    option.to_uppercase(),
                    value
                )],
            )
        }
        Err(crate::db::DbError::UnknownOption(opt)) => reply_effects(
            uid,
            vec![&format!(
                "Unknown option: \x02{}\x02. Valid options: EMAIL, ENFORCE, HIDEMAIL, PASSWORD",
                opt
            )],
        ),
        Err(e) => {
            warn!(account = %account.name, option = %option, error = ?e, "SET failed");
            reply_effects(uid, vec!["Failed to update setting."])
        }
    }
}
