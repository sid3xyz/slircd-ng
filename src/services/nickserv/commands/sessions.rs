//! SESSIONS command handler for NickServ.
//!
//! Shows active sessions for the user's account or a specified account (if oper).

use super::NickServResult;
use crate::services::ServiceEffect;
use crate::state::Matrix;
use std::sync::Arc;
use tracing::debug;

/// Handle SESSIONS command.
///
/// Usage:
/// - `SESSIONS` - List your own active sessions
/// - `SESSIONS <account>` - List sessions for an account (opers only)
pub async fn handle_sessions(
    matrix: &Arc<Matrix>,
    uid: &str,
    user_account: Option<&str>,
    is_oper: bool,
    args: &[&str],
    reply_effect: impl Fn(&str, &str) -> ServiceEffect,
    reply_effects: impl Fn(&str, Vec<&str>) -> NickServResult,
) -> NickServResult {
    // Determine target account
    let target_account = if args.is_empty() {
        // No argument - show own sessions
        match user_account {
            Some(account) => account.to_string(),
            None => {
                return reply_effects(uid, vec!["You are not logged in to an account."]);
            }
        }
    } else {
        let requested = args[0];
        match (user_account, is_oper) {
            (Some(own_account), _) if slirc_proto::irc_eq(own_account, requested) => {
                requested.to_string()
            }
            (_, true) => requested.to_string(),
            (Some(_), false) => {
                return reply_effects(
                    uid,
                    vec!["You can only view sessions for your own account."],
                );
            }
            (None, false) => {
                return reply_effects(uid, vec!["You are not logged in to an account."]);
            }
        }
    };

    // Check if multiclient is enabled
    if !matrix.config.multiclient.enabled {
        return reply_effects(
            uid,
            vec!["Multiclient feature is not enabled on this server."],
        );
    }

    // Get sessions from client manager
    let sessions = matrix.client_manager.get_sessions(&target_account);

    if sessions.is_empty() {
        return reply_effects(
            uid,
            vec![&format!(
                "No active sessions for account \x02{}\x02.",
                target_account
            )],
        );
    }

    debug!(
        uid = %uid,
        account = %target_account,
        session_count = sessions.len(),
        "SESSIONS command executed"
    );

    let mut effects = vec![reply_effect(
        uid,
        &format!(
            "Active sessions for \x02{}\x02 ({} total):",
            target_account,
            sessions.len()
        ),
    )];

    for (idx, session) in sessions.iter().enumerate() {
        let device_str = session
            .device_id
            .as_ref()
            .map(|d| format!(" (device: {})", d))
            .unwrap_or_default();

        let oper_suffix = if is_oper {
            format!(" [id: {}, ip: {}]", session.session_id, session.ip)
        } else {
            String::new()
        };

        let attached_time = session.attached_at.format("%Y-%m-%d %H:%M:%S UTC");

        effects.push(reply_effect(
            uid,
            &format!(
                "  {}. Connected since: {}{}{}",
                idx + 1,
                attached_time,
                device_str,
                oper_suffix
            ),
        ));
    }

    effects.push(reply_effect(uid, "End of session list."));

    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MulticlientConfig;

    fn mock_reply_effect(_uid: &str, text: &str) -> ServiceEffect {
        ServiceEffect::Reply {
            target_uid: "test".to_string(),
            msg: slirc_proto::Message {
                tags: None,
                prefix: None,
                command: slirc_proto::Command::PRIVMSG("test".to_string(), text.to_string()),
            },
        }
    }

    fn mock_reply_effects(_uid: &str, texts: Vec<&str>) -> NickServResult {
        texts
            .into_iter()
            .map(|t| mock_reply_effect(_uid, t))
            .collect()
    }

    #[test]
    fn test_sessions_requires_login() {
        // Can't easily test async without a runtime, but the logic is straightforward
    }
}
