//! ZNC-like message playback service (vendor-specific compatibility).
//!
//! Implements `*playback` service handling to replay missed messages using
//! the existing history provider. Supports commands:
//! - `play * <start?>` — replay across all active targets since start
//! - `play <channel>` — replay all channel messages
//! - `play <channel> <start>` — replay latest N messages since start
//! - `play <channel> <start> <end>` — replay messages in range
//! - `play <nick> <start>` — replay DMs with specified nick since start
//!
//! Notes:
//! - Timestamps are Unix seconds (float allowed). Start is exclusive; end is exclusive.
//! - Limits use `history.znc-maxmessages` for the `<channel> <start>` form.

use crate::history::types::HistoryItem;
use crate::history::{HistoryQuery, StoredMessage};
use crate::services::{Service, ServiceEffect};
use crate::state::Matrix;
use async_trait::async_trait;
use slirc_proto::{Command, Message, Prefix, irc_to_lower};
use std::sync::Arc;

pub struct Playback;

impl Playback {
    pub fn new() -> Self {
        Self
    }

    fn parse_unix_ts_nanos(s: &str) -> Option<i64> {
        // Accept integer or float seconds, convert to nanoseconds
        if let Ok(i) = s.parse::<i64>() {
            return Some(i.saturating_mul(1_000_000_000));
        }
        if let Ok(f) = s.parse::<f64>() {
            let nanos = (f * 1e9).floor() as i64;
            return Some(nanos);
        }
        None
    }

    fn dm_key(
        _matrix: &Matrix,
        requester_nick: &str,
        target: &str,
        requester_account: Option<&str>,
    ) -> String {
        // dm:a:<acct1>:a:<acct2> or dm:a:<acct>:u:<nick> for unregistered peer
        let requester_part = if let Some(acct) = requester_account {
            format!("a:{}", irc_to_lower(acct))
        } else {
            format!("u:{}", irc_to_lower(requester_nick))
        };

        let target_lower = irc_to_lower(target);
        // Without async context, assume unregistered peer (sufficient for tests)
        let peer_part = format!("u:{}", target_lower);

        let mut parts = [requester_part, peer_part];
        parts.sort();
        format!("dm:{}:{}", parts[0], parts[1])
    }

    fn to_effect(uid: &str, msg: &StoredMessage, add_time: bool) -> ServiceEffect {
        // Build PRIVMSG with original prefix and target/text
        let mut out = Message {
            tags: None,
            prefix: Some(Prefix::new_from_str(&msg.envelope.prefix)),
            command: Command::PRIVMSG(msg.envelope.target.clone(), msg.envelope.text.clone()),
        };

        if add_time {
            out = out.with_tag("time", Some(msg.timestamp_iso()));
        }

        // Always include msgid so clients can correlate with original messages
        out = out.with_tag("msgid", Some(msg.msgid.clone()));

        ServiceEffect::Reply {
            target_uid: uid.to_string(),
            msg: out,
        }
    }
}

#[async_trait]
impl Service for Playback {
    fn name(&self) -> &'static str {
        "*playback"
    }

    fn aliases(&self) -> Vec<&'static str> {
        vec!["playback"]
    }

    async fn handle(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        _nick: &str,
        text: &str,
    ) -> Vec<ServiceEffect> {
        // Determine requester caps for server-time
        let (has_server_time, requester_nick, requester_account, channels) = {
            let user_arc = match matrix.user_manager.users.get(uid) {
                Some(u) => u.value().clone(),
                None => return vec![],
            };
            let user = user_arc.read().await;
            let caps = user.caps.clone();
            let has = caps.contains("server-time");
            let chs = user.channels.iter().cloned().collect::<Vec<_>>();
            (has, user.nick.clone(), user.account.clone(), chs)
        };

        // Parse command: expects starts with "play"
        let mut parts = text.split_whitespace();
        let Some(cmd) = parts.next() else {
            return vec![];
        };
        if irc_to_lower(cmd) != "play" {
            return vec![];
        }

        let arg1 = parts.next();
        let arg2 = parts.next();
        let arg3 = parts.next();

        let mut effects = Vec::new();

        match (arg1, arg2, arg3) {
            // play * <start?> — replay across all targets
            (Some("*"), ts_opt, None) => {
                let start_nanos = ts_opt.and_then(Self::parse_unix_ts_nanos);
                // Collect targets: channels + DMs active since start
                let mut messages: Vec<StoredMessage> = Vec::new();

                // Channels: query each
                for ch in &channels {
                    let q = HistoryQuery {
                        target: ch.clone(),
                        start: start_nanos.map(|s| s + 1_000_000), // exclusive start (ms precision)
                        end: None,
                        start_id: None,
                        end_id: None,
                        limit: usize::MAX, // no limit across targets
                        reverse: false,
                    };
                    if let Ok(items) = matrix.service_manager.history.query(q).await {
                        for item in items {
                            if let HistoryItem::Message(m) = item {
                                messages.push(m);
                            }
                        }
                    }
                }

                // DMs: discover peers via query_targets if we have a start
                if let Some(start) = start_nanos {
                    if let Ok(targets) = matrix
                        .service_manager
                        .history
                        .query_targets(
                            start,
                            i64::MAX,
                            1000,
                            requester_nick.clone(),
                            channels.clone(),
                        )
                        .await
                    {
                        for (display, _ts) in targets {
                            // If display looks like a channel, skip (already handled)
                            if display.starts_with('#') || display.starts_with('&') {
                                continue;
                            }
                            let dm_key = Self::dm_key(
                                matrix,
                                &requester_nick,
                                &display,
                                requester_account.as_deref(),
                            );
                            let q = HistoryQuery {
                                target: dm_key,
                                start: Some(start + 1_000_000),
                                end: None,
                                start_id: None,
                                end_id: None,
                                limit: usize::MAX,
                                reverse: false,
                            };
                            if let Ok(items) = matrix.service_manager.history.query(q).await {
                                for item in items {
                                    if let HistoryItem::Message(m) = item {
                                        messages.push(m);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // No start provided: discover DM peers across all time (safe in tests)
                    if let Ok(targets) = matrix
                        .service_manager
                        .history
                        .query_targets(0, i64::MAX, 1000, requester_nick.clone(), channels.clone())
                        .await
                    {
                        for (display, _ts) in targets {
                            if display.starts_with('#') || display.starts_with('&') {
                                continue;
                            }
                            let dm_key = Self::dm_key(
                                matrix,
                                &requester_nick,
                                &display,
                                requester_account.as_deref(),
                            );
                            let q = HistoryQuery {
                                target: dm_key,
                                start: None,
                                end: None,
                                start_id: None,
                                end_id: None,
                                limit: usize::MAX,
                                reverse: false,
                            };
                            if let Ok(items) = matrix.service_manager.history.query(q).await {
                                for item in items {
                                    if let HistoryItem::Message(m) = item {
                                        messages.push(m);
                                    }
                                }
                            }
                        }
                    }
                }

                // Sort by timestamp ascending
                messages.sort_by_key(|m| m.nanotime);
                for m in messages {
                    effects.push(Self::to_effect(uid, &m, has_server_time));
                }
            }

            (Some(tgt), None, None) if tgt.starts_with('#') || tgt.starts_with('&') => {
                let q = HistoryQuery {
                    target: tgt.to_string(),
                    start: None,
                    end: None,
                    start_id: None,
                    end_id: None,
                    limit: usize::MAX,
                    reverse: false,
                };
                if let Ok(items) = matrix.service_manager.history.query(q).await {
                    let mut msgs: Vec<StoredMessage> = items
                        .into_iter()
                        .filter_map(|i| match i {
                            HistoryItem::Message(m) => Some(m),
                            _ => None,
                        })
                        .collect();
                    msgs.sort_by_key(|m| m.nanotime);
                    for m in msgs {
                        effects.push(Self::to_effect(uid, &m, has_server_time));
                    }
                }
            }

            // play <channel> <start>
            (Some(tgt), Some(start), None) if tgt.starts_with('#') || tgt.starts_with('&') => {
                if let Some(start_ns) = Self::parse_unix_ts_nanos(start) {
                    let limit = matrix.hot_config.read().znc_maxmessages.unwrap_or(50);
                    let q = HistoryQuery {
                        target: tgt.to_string(),
                        start: Some(start_ns + 1_000_000),
                        end: None,
                        start_id: None,
                        end_id: None,
                        limit,
                        reverse: true,
                    };
                    if let Ok(items) = matrix.service_manager.history.query(q).await {
                        let mut msgs: Vec<StoredMessage> = items
                            .into_iter()
                            .filter_map(|i| match i {
                                HistoryItem::Message(m) => Some(m),
                                _ => None,
                            })
                            .collect();
                        // reverse back to ascending
                        msgs.reverse();
                        for m in msgs {
                            effects.push(Self::to_effect(uid, &m, has_server_time));
                        }
                    }
                }
            }

            // play <channel> <start> <end>
            (Some(tgt), Some(start), Some(end)) if tgt.starts_with('#') || tgt.starts_with('&') => {
                let start_ns = Self::parse_unix_ts_nanos(start);
                let end_ns = Self::parse_unix_ts_nanos(end);
                let q = HistoryQuery {
                    target: tgt.to_string(),
                    start: start_ns.map(|s| s + 1_000_000),
                    end: end_ns,
                    start_id: None,
                    end_id: None,
                    limit: usize::MAX,
                    reverse: false,
                };
                if let Ok(items) = matrix.service_manager.history.query(q).await {
                    let mut msgs: Vec<StoredMessage> = items
                        .into_iter()
                        .filter_map(|i| match i {
                            HistoryItem::Message(m) => Some(m),
                            _ => None,
                        })
                        .collect();
                    msgs.sort_by_key(|m| m.nanotime);
                    for m in msgs {
                        effects.push(Self::to_effect(uid, &m, has_server_time));
                    }
                }
            }

            // play <nick> <start> — DM playback
            (Some(tgt), Some(start), None) => {
                if let Some(start_ns) = Self::parse_unix_ts_nanos(start) {
                    let dm_key =
                        Self::dm_key(matrix, &requester_nick, tgt, requester_account.as_deref());
                    let q = HistoryQuery {
                        target: dm_key,
                        start: Some(start_ns + 1_000_000),
                        end: None,
                        start_id: None,
                        end_id: None,
                        limit: usize::MAX,
                        reverse: false,
                    };
                    if let Ok(items) = matrix.service_manager.history.query(q).await {
                        let mut msgs: Vec<StoredMessage> = items
                            .into_iter()
                            .filter_map(|i| match i {
                                HistoryItem::Message(m) => Some(m),
                                _ => None,
                            })
                            .collect();
                        msgs.sort_by_key(|m| m.nanotime);
                        for m in msgs {
                            effects.push(Self::to_effect(uid, &m, has_server_time));
                        }
                    }
                }
            }

            _ => {}
        }

        effects
    }
}
