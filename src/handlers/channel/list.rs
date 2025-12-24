//! LIST command handler.

use super::super::{Context, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Parse ELIST filters from LIST parameter.
#[derive(Debug, Default)]
struct ListFilter {
    mask: Option<String>,
    negative_mask: Option<String>,
    min_users: Option<usize>,
    max_users: Option<usize>,
}

impl ListFilter {
    fn parse(arg: Option<&str>) -> Self {
        let Some(filter_str) = arg else {
            return Self::default();
        };

        let mut filter = Self::default();

        // ELIST=U: >N or <N user count filtering
        if let Some(num_str) = filter_str.strip_prefix('>') {
            if let Ok(count) = num_str.parse() {
                filter.min_users = Some(count);
            }
            return filter;
        }
        if let Some(num_str) = filter_str.strip_prefix('<') {
            if let Ok(count) = num_str.parse() {
                filter.max_users = Some(count);
            }
            return filter;
        }

        // ELIST=N: !pattern (negative mask)
        if let Some(pattern) = filter_str.strip_prefix('!') {
            filter.negative_mask = Some(pattern.to_string());
            return filter;
        }

        // ELIST=M: pattern (positive mask) or exact channel name
        filter.mask = Some(filter_str.to_string());
        filter
    }

    fn matches(&self, name: &str, member_count: usize) -> bool {
        // User count filters
        if let Some(min) = self.min_users
            && member_count <= min
        {
            return false;
        }
        if let Some(max) = self.max_users
            && member_count >= max
        {
            return false;
        }

        // Negative mask (ELIST=N)
        if let Some(ref neg_mask) = self.negative_mask
            && wildcard_match(neg_mask, name)
        {
            return false;
        }

        // Positive mask (ELIST=M) or exact match
        if let Some(ref mask) = self.mask {
            return wildcard_match(mask, name);
        }

        true
    }
}

/// Simple wildcard matcher for IRC channel names.
/// Supports * (any chars) and ? (single char).
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = None;
    let mut star_ti = None;

    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'*') {
            star_pi = Some(pi);
            star_ti = Some(ti);
            pi += 1;
        } else if pi < pattern.len()
            && (pattern[pi] == b'?' || pattern[pi].eq_ignore_ascii_case(&text[ti]))
        {
            pi += 1;
            ti += 1;
        } else if let Some(s_pi) = star_pi {
            pi = s_pi + 1;
            // SAFETY: star_ti is always Some when star_pi is Some - both set together on lines 91-92
            let new_ti = star_ti.expect("star_ti set with star_pi") + 1;
            star_ti = Some(new_ti);
            ti = new_ti;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// Handler for LIST command.
///
/// `LIST [channels [target]]`
///
/// Lists channels and their topics.
/// # RFC 2812 §3.2.6
///
/// List message - Lists channels and their topics.
///
/// **Specification:** [RFC 2812 §3.2.6](https://datatracker.ietf.org/doc/html/rfc2812#section-3.2.6)
///
/// **Compliance:** 3/8 irctest pass (5 ELIST extensions skipped)
pub struct ListHandler;

#[async_trait]
impl PostRegHandler for ListHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = ctx.server_name();
        let nick = &ctx.state.nick;

        // Parse ELIST filters from argument
        let filter = ListFilter::parse(msg.arg(0));

        // RPL_LISTSTART (321): Channel :Users Name (optional, some clients don't expect it)

        // Collect channel senders first to avoid holding DashMap lock across await points
        // This prevents deadlocks if the actor tries to access the channel map
        let channels: Vec<_> = ctx
            .matrix
            .channel_manager
            .channels
            .iter()
            .map(|r| r.value().clone())
            .collect();

        // Result limiting to prevent flooding
        let max_channels = ctx.matrix.config.limits.max_list_channels;
        let mut result_count = 0;
        let mut truncated = false;

        // Iterate channels
        for sender in channels {
            // Check result limit
            if result_count >= max_channels {
                truncated = true;
                break;
            }

            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = sender
                .send(crate::state::actor::ChannelEvent::GetInfo {
                    requester_uid: Some(ctx.uid.to_string()),
                    reply_tx: tx,
                })
                .await;

            let channel = match rx.await {
                Ok(info) => info,
                Err(_) => continue,
            };

            // Skip secret channels unless user is a member
            if channel
                .modes
                .contains(&crate::state::actor::ChannelMode::Secret)
                && !channel.is_member
            {
                continue;
            }

            // Apply ELIST filters
            if !filter.matches(&channel.name, channel.member_count) {
                continue;
            }

            let topic_text = channel
                .topic
                .as_ref()
                .map(|t| t.text.clone())
                .unwrap_or_default();

            // RPL_LIST (322): <channel> <# visible> :<topic>
            let reply = server_reply(
                server_name,
                Response::RPL_LIST,
                vec![
                    nick.clone(),
                    channel.name.clone(),
                    channel.member_count.to_string(),
                    topic_text,
                ],
            );
            ctx.sender.send(reply).await?;
            result_count += 1;
        }

        // Notify if results were truncated
        if truncated {
            let notice = server_reply(
                server_name,
                Response::RPL_TRYAGAIN,
                vec![
                    nick.clone(),
                    "LIST".to_string(),
                    format!("Output truncated, {} channels max", max_channels),
                ],
            );
            ctx.sender.send(notice).await?;
        }

        // RPL_LISTEND (323): :End of LIST
        let reply = server_reply(
            server_name,
            Response::RPL_LISTEND,
            vec![nick.clone(), "End of LIST".to_string()],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wildcard_match() {
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("foo*", "foobar"));
        assert!(wildcard_match("*bar", "foobar"));
        assert!(wildcard_match("f?o", "foo"));
        assert!(wildcard_match("f?o", "fOo"));
        assert!(!wildcard_match("foo", "bar"));
    }

    #[test]
    fn test_list_filter_parse() {
        let f = ListFilter::parse(None);
        assert!(f.mask.is_none());
        assert!(f.min_users.is_none());
        assert!(f.max_users.is_none());

        let f = ListFilter::parse(Some(">10"));
        assert_eq!(f.min_users, Some(10));

        let f = ListFilter::parse(Some("<5"));
        assert_eq!(f.max_users, Some(5));

        let f = ListFilter::parse(Some("!*bad*"));
        assert_eq!(f.negative_mask, Some("*bad*".to_string()));

        let f = ListFilter::parse(Some("#channel"));
        assert_eq!(f.mask, Some("#channel".to_string()));
    }

    #[test]
    fn test_list_filter_matches() {
        let mut f = ListFilter::default();
        f.min_users = Some(5);
        assert!(!f.matches("#chan", 5));
        assert!(f.matches("#chan", 6));

        let mut f = ListFilter::default();
        f.max_users = Some(5);
        assert!(!f.matches("#chan", 5));
        assert!(f.matches("#chan", 4));

        let mut f = ListFilter::default();
        f.mask = Some("#chan*".to_string());
        assert!(f.matches("#channel", 10));
        assert!(!f.matches("#other", 10));

        let mut f = ListFilter::default();
        f.negative_mask = Some("*bad*".to_string());
        assert!(!f.matches("#badchan", 10));
        assert!(f.matches("#goodchan", 10));
    }
}
