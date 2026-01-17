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
            reply_effect(
                uid,
                "  ALWAYS-ON ON|OFF - Keep presence when all sessions disconnect",
            ),
            reply_effect(
                uid,
                "  AUTO-AWAY ON|OFF - Set away when all sessions disconnect",
            ),
            reply_effect(uid, "  EMAIL <address>  - Set email address"),
            reply_effect(
                uid,
                "  ENFORCE ON|OFF   - Enable/disable nickname enforcement",
            ),
            reply_effect(uid, "  HIDEMAIL ON|OFF  - Hide/show email in INFO"),
            reply_effect(uid, "  PASSWORD <pass>  - Change password"),
        ];
    }

    // Check if user is identified and get their account name
    let user_arc = matrix
        .user_manager
        .users
        .get(uid)
        .map(|u| u.value().clone());
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

    let option = args[0].to_uppercase();
    let value = args[1];

    // Handle bouncer-specific options in memory (ClientManager)
    match option.as_str() {
        "MULTICLIENT" => {
            let enabled = match value.to_uppercase().as_str() {
                "ON" | "TRUE" | "1" | "YES" => true,
                "OFF" | "FALSE" | "0" | "NO" => false,
                _ => {
                    return reply_effects(uid, vec!["Value must be ON or OFF."]);
                }
            };

            // Update override in ClientManager
            matrix
                .client_manager
                .set_multiclient_override(&account_name, enabled);
            info!(account = %account_name, enabled = enabled, "MULTICLIENT setting changed");

            return reply_effects(
                uid,
                vec![&format!(
                    "\x02MULTICLIENT\x02 has been set to \x02{}\x02.",
                    if enabled { "ON" } else { "OFF" }
                )],
            );
        }
        "ALWAYS-ON" => {
            let enabled = match value.to_uppercase().as_str() {
                "ON" | "TRUE" | "1" | "YES" => true,
                "OFF" | "FALSE" | "0" | "NO" => false,
                _ => {
                    return reply_effects(uid, vec!["Value must be ON or OFF."]);
                }
            };

            // Check if always-on is allowed by server policy
            if enabled {
                use crate::config::AlwaysOnPolicy;
                match matrix.config.multiclient.always_on {
                    AlwaysOnPolicy::Disabled => {
                        return reply_effects(uid, vec!["Always-on is disabled on this server."]);
                    }
                    AlwaysOnPolicy::OptIn | AlwaysOnPolicy::OptOut | AlwaysOnPolicy::Mandatory => {
                        // Allowed
                    }
                }
            }

            // Update in ClientManager
            if let Some(client) = matrix.client_manager.get_client(&account_name) {
                let mut client_guard = client.write().await;
                client_guard.set_always_on(enabled);
                info!(account = %account_name, enabled = enabled, "ALWAYS-ON setting changed");

                // Trigger persistence
                drop(client_guard);
                matrix.client_manager.persist_client(&account_name).await;

                return reply_effects(
                    uid,
                    vec![&format!(
                        "\x02ALWAYS-ON\x02 has been set to \x02{}\x02.",
                        if enabled { "ON" } else { "OFF" }
                    )],
                );
            } else {
                return reply_effects(
                    uid,
                    vec!["No bouncer session found. Connect again after identifying."],
                );
            }
        }
        "AUTO-AWAY" => {
            let enabled = match value.to_uppercase().as_str() {
                "ON" | "TRUE" | "1" | "YES" => true,
                "OFF" | "FALSE" | "0" | "NO" => false,
                _ => {
                    return reply_effects(uid, vec!["Value must be ON or OFF."]);
                }
            };

            // Update in ClientManager
            if let Some(client) = matrix.client_manager.get_client(&account_name) {
                let mut client_guard = client.write().await;
                client_guard.set_auto_away(enabled);
                info!(account = %account_name, enabled = enabled, "AUTO-AWAY setting changed");

                // Trigger persistence
                drop(client_guard);
                matrix.client_manager.persist_client(&account_name).await;

                return reply_effects(
                    uid,
                    vec![&format!(
                        "\x02AUTO-AWAY\x02 has been set to \x02{}\x02.",
                        if enabled { "ON" } else { "OFF" }
                    )],
                );
            } else {
                return reply_effects(
                    uid,
                    vec!["No bouncer session found. Connect again after identifying."],
                );
            }
        }
        _ => {
            // Fall through to database-backed options
        }
    }

    // Handle database-backed options
    match db.accounts().set_option(account.id, &option, value).await {
        Ok(()) => {
            info!(account = %account.name, option = %option, "Account setting changed");
            reply_effects(
                uid,
                vec![&format!(
                    "\x02{}\x02 has been set to \x02{}\x02.",
                    option, value
                )],
            )
        }
        Err(crate::db::DbError::UnknownOption(opt)) => reply_effects(
            uid,
            vec![&format!(
                "Unknown option: \x02{}\x02. Valid options: MULTICLIENT, ALWAYS-ON, AUTO-AWAY, EMAIL, ENFORCE, HIDEMAIL, PASSWORD",
                opt
            )],
        ),
        Err(e) => {
            warn!(account = %account.name, option = %option, error = ?e, "SET failed");
            reply_effects(uid, vec!["Failed to update setting."])
        }
    }
}
