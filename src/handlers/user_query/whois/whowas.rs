//! WHOWAS handler for historical user information.

use crate::handlers::{Context, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
use async_trait::async_trait;
use regex::Regex;
use slirc_proto::{MessageRef, Response, irc_to_lower};
use std::sync::LazyLock;

/// Pre-compiled empty regex for fallback - cannot panic.
static EMPTY_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    // SAFETY: "^$" is a compile-time constant literal that is always valid regex
    Regex::new("^$").expect("empty regex pattern is always valid")
});

/// Convert a glob pattern (with * and ?) to a regex.
/// Returns a reference to a static empty regex on compilation failure.
fn glob_to_regex(pattern: &str) -> Regex {
    let mut regex_str = String::from("^");
    for c in pattern.chars() {
        match c {
            '*' => regex_str.push_str(".*"),
            '?' => regex_str.push('.'),
            '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                regex_str.push('\\');
                regex_str.push(c);
            }
            _ => regex_str.push(c),
        }
    }
    regex_str.push('$');
    Regex::new(&regex_str).unwrap_or_else(|_| EMPTY_REGEX.clone())
}

/// Handler for WHOWAS command.
///
/// `WHOWAS nickname [count [server]]`
///
/// Returns information about a nickname that no longer exists.
/// Queries the WHOWAS history stored in Matrix.
pub struct WhowasHandler;

#[async_trait]
impl PostRegHandler for WhowasHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // WHOWAS <nick>[,<nick>] [count [server]]
        let targets = msg.arg(0).unwrap_or("");

        // Parse count parameter
        // Per RFC 1459/2812 and Modern IRC spec:
        // "If a non-positive number is passed as being <count>, then a full search is done"
        let count_limit: Option<usize> = msg
            .arg(1)
            .and_then(|s| s.parse::<i32>().ok())
            .and_then(|n| {
                if n <= 0 {
                    None // Full search - no limit
                } else {
                    Some((n as usize).min(10)) // Cap positive values at 10
                }
            })
            .or(Some(10)); // Default to 10 if not specified

        if targets.is_empty() {
            let reply = server_reply(
                ctx.server_name(),
                Response::ERR_NONICKNAMEGIVEN,
                vec![
                    ctx.state.nick.clone(),
                    "No nickname given".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;

            // Per RFC 2812, WHOWAS with no params should still send ENDOFWHOWAS
            // Use a placeholder nick for the end message
            let end_reply = server_reply(
                ctx.server_name(),
                Response::RPL_ENDOFWHOWAS,
                vec![
                    ctx.state.nick.clone(),
                    "*".to_string(),
                    "End of WHOWAS".to_string(),
                ],
            );
            ctx.sender.send(end_reply).await?;
            return Ok(());
        }

        let server_name = ctx.server_name();
        let nick = &ctx.state.nick; // Guaranteed present in RegisteredState

        // Handle multiple targets (comma-separated): WHOWAS nick1,nick2
        let target_list: Vec<&str> = targets.split(',').map(|s| s.trim()).collect();

        // Track the first nick found for ENDOFWHOWAS reply
        let mut first_matched_nick: Option<String> = None;

        // Collect all entries from all targets, then sort by logout_time (most recent first)
        let mut all_entries: Vec<crate::state::WhowasEntry> = Vec::with_capacity(16);

        for target in &target_list {
            let target_lower = irc_to_lower(target);

            // Check if target contains wildcards
            if target.contains('*') || target.contains('?') {
                // Wildcard match - search all entries
                let pattern = glob_to_regex(&target_lower);
                for entry in ctx.matrix.user_manager.whowas.iter() {
                    if pattern.is_match(entry.key()) {
                        for e in entry.value().iter() {
                            if first_matched_nick.is_none() {
                                first_matched_nick = Some(e.nick.clone());
                            }
                            all_entries.push(e.clone());
                        }
                    }
                }
            } else if let Some(entries) = ctx.matrix.user_manager.whowas.get(&target_lower) {
                for entry in entries.iter() {
                    if first_matched_nick.is_none() {
                        first_matched_nick = Some(entry.nick.clone());
                    }
                    all_entries.push(entry.clone());
                }
            }
        }

        // Sort by logout_time (most recent first)
        all_entries.sort_by(|a, b| b.logout_time.cmp(&a.logout_time));

        // Apply count limit if specified
        let entries_to_show: Vec<_> = if let Some(limit) = count_limit {
            all_entries.into_iter().take(limit).collect()
        } else {
            all_entries
        };

        if !entries_to_show.is_empty() {
            // Send RPL_WHOWASUSER for each entry
            for entry in entries_to_show {
                // RPL_WHOWASUSER (314): <nick> <user> <host> * :<realname>
                let reply = server_reply(
                    server_name,
                    Response::RPL_WHOWASUSER,
                    vec![
                        nick.clone(),
                        entry.nick.clone(),
                        entry.user,
                        entry.host,
                        "*".to_string(),
                        entry.realname,
                    ],
                );
                ctx.sender.send(reply).await?;

                // RPL_WHOISSERVER (312): <nick> <server> :<server info>
                let reply = server_reply(
                    server_name,
                    Response::RPL_WHOISSERVER,
                    vec![
                        nick.clone(),
                        entry.nick.clone(),
                        entry.server,
                        format!("Logged out at {}", format_timestamp(entry.logout_time)),
                    ],
                );
                ctx.sender.send(reply).await?;
            }
        } else {
            // No history found for any target
            let reply = server_reply(
                server_name,
                Response::ERR_WASNOSUCHNICK,
                vec![
                    nick.clone(),
                    targets.to_string(),
                    "There was no such nickname".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
        }

        // RPL_ENDOFWHOWAS (369): <requester> <nick> :End of WHOWAS
        // For multi-target queries, use the original query string
        // For single-target wildcard queries, use the matched nick if found
        // For single-target exact queries, use the query string
        let is_multi_target = target_list.len() > 1;
        let is_wildcard = target_list.len() == 1
            && (targets.contains('*') || targets.contains('?'));

        let end_nick = if is_multi_target {
            // Multi-target: always use original query string
            targets.to_string()
        } else if is_wildcard {
            // Wildcard: use matched nick if found, otherwise query string
            first_matched_nick.unwrap_or_else(|| targets.to_string())
        } else {
            // Exact match: use query string
            targets.to_string()
        };
        let reply = server_reply(
            server_name,
            Response::RPL_ENDOFWHOWAS,
            vec![
                nick.clone(),
                end_nick,
                "End of WHOWAS".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Format a Unix timestamp (milliseconds) as a human-readable string.
fn format_timestamp(ts_millis: i64) -> String {
    let ts_secs = ts_millis / 1000;
    chrono::DateTime::from_timestamp(ts_secs, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
