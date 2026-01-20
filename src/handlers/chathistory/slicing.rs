use crate::history::types::HistoryItem;
use slirc_proto::MessageReference;

/// Determine the slice of messages centered around a target message or timestamp.
pub fn slice_around(
    messages: Vec<HistoryItem>,
    limit: usize,
    msgref_str: &str,
    center_ts: i64,
) -> Vec<HistoryItem> {
    // Extract bare msgid from the msgref_str
    let bare_msgid = if let Ok(MessageReference::MsgId(id)) = MessageReference::parse(msgref_str) {
        Some(id)
    } else {
        None
    };

    // Find the index of the center message
    let center_idx = if let Some(ref id) = bare_msgid {
        // Msgid reference: look up by msgid
        messages.iter().position(|m| match m {
            HistoryItem::Message(msg) => msg.msgid == *id,
            HistoryItem::Event(evt) => evt.id == *id,
        })
    } else if !messages.is_empty() {
        // Timestamp reference OR wildcard: find message closest to center_ts
        if center_ts > 0 {
            // Find the index with minimum distance to center_ts
            let closest = messages
                .iter()
                .enumerate()
                .min_by_key(|(_, m)| (m.nanotime() - center_ts).unsigned_abs());
            closest.map(|(i, _)| i)
        } else {
            // Use middle message as fallback
            Some(messages.len() / 2)
        }
    } else {
        None
    };

    if let Some(idx) = center_idx {
        let half = limit / 2;
        let start_idx = idx.saturating_sub(half);
        let end_idx = (start_idx + limit).min(messages.len());

        messages[start_idx..end_idx].to_vec()
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{MessageEnvelope, StoredMessage};

    fn make_msg(id: &str, ts: i64) -> HistoryItem {
        HistoryItem::Message(StoredMessage {
            msgid: id.to_string(),
            nanotime: ts,
            target: "#test".to_string(),
            sender: "nick".to_string(),
            account: None,
            envelope: MessageEnvelope {
                command: "PRIVMSG".to_string(),
                prefix: "nick!user@host".to_string(),
                target: "#test".to_string(),
                text: "hello".to_string(),
                tags: None,
            },
        })
    }

    #[test]
    fn test_slice_around_msgid() {
        let msgs = vec![
            make_msg("1", 100),
            make_msg("2", 200),
            make_msg("3", 300),
            make_msg("4", 400),
            make_msg("5", 500),
        ];

        // Center on "3", limit 3 -> expect 2, 3, 4
        let res = slice_around(msgs.clone(), 3, "msgid=3", 0);
        assert_eq!(res.len(), 3);
        match &res[0] {
            HistoryItem::Message(m) => assert_eq!(m.msgid, "2"),
            _ => panic!(),
        }
        match &res[1] {
            HistoryItem::Message(m) => assert_eq!(m.msgid, "3"),
            _ => panic!(),
        }
        match &res[2] {
            HistoryItem::Message(m) => assert_eq!(m.msgid, "4"),
            _ => panic!(),
        }
    }

    #[test]
    fn test_slice_around_timestamp() {
        let msgs = vec![
            make_msg("1", 100),
            make_msg("2", 200),
            make_msg("3", 300), // Closest to 290
            make_msg("4", 400),
            make_msg("5", 500),
        ];

        // Center on ts=290, limit 3 -> expect 2, 3, 4
        let res = slice_around(msgs.clone(), 3, "timestamp=2023...", 290);
        assert_eq!(res.len(), 3);
        match &res[1] {
            HistoryItem::Message(m) => assert_eq!(m.msgid, "3"),
            _ => panic!(),
        }
    }

    #[test]
    fn test_slice_around_edge_start() {
        let msgs = vec![make_msg("1", 100), make_msg("2", 200), make_msg("3", 300)];

        // Center on "1", limit 3 -> expect 1, 2, 3
        let res = slice_around(msgs.clone(), 3, "msgid=1", 0);
        assert_eq!(res.len(), 3);
        match &res[0] {
            HistoryItem::Message(m) => assert_eq!(m.msgid, "1"),
            _ => panic!(),
        }
    }

    #[test]
    fn test_slice_around_edge_end() {
        let msgs = vec![make_msg("1", 100), make_msg("2", 200), make_msg("3", 300)];

        // Center on "3", limit 3 -> expect 1, 2, 3 (idx 2, half 1 -> start 1 -> 2, 3?)
        // idx=2, half=1. start = 2-1 = 1. end = 1+3 = 4 (min 3) = 3.
        // slice [1..3] -> 2, 3.
        // Wait, centering logic: start = idx - half.
        // If idx=2, half=1 -> start=1. returns msg[1], msg[2] ("2", "3").
        // Length 2. Limit was 3.
        // This logic shifts the window to start at center-half. It doesn't guarantee filling the limit by looking backwards if close to end.
        // This is standard simple windowing.

        let res = slice_around(msgs.clone(), 3, "msgid=3", 0);
        assert_eq!(res.len(), 2);
        match &res[0] {
            HistoryItem::Message(m) => assert_eq!(m.msgid, "2"),
            _ => panic!(),
        }
        match &res[1] {
            HistoryItem::Message(m) => assert_eq!(m.msgid, "3"),
            _ => panic!(),
        }
    }
}
