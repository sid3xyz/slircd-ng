//! Messaging command handlers (PRIVMSG, NOTICE, TAGMSG).
//!
//! Handles message routing to users and channels with support for:
//! - Channel modes (+n, +m, +r)
//! - Ban lists and quiet lists
//! - CTCP protocol
//! - Spam detection
//! - Service integration (NickServ, ChanServ)
//! - Event-sourced history (Innovation 5)

mod accept;

mod delivery;
mod errors;
mod metadata;
mod multiclient;
mod notice;

mod privmsg;
mod relaymsg;
mod routing;

mod tagmsg;
mod types;
mod validation;

pub use accept::AcceptHandler;
pub use metadata::MetadataHandler;
pub use notice::NoticeHandler;
pub use privmsg::PrivmsgHandler;
pub use relaymsg::RelayMsgHandler;

// ============================================================================
// TAGMSG Handler
// ============================================================================

pub use tagmsg::TagmsgHandler;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::handlers::matches_hostmask;
    use slirc_proto::ChannelExt;

    #[test]
    fn test_is_channel() {
        assert!("#rust".is_channel_name());
        assert!("&local".is_channel_name());
        assert!("+modeless".is_channel_name());
        assert!("!safe".is_channel_name());
        assert!(!"nickname".is_channel_name());
        assert!(!"NickServ".is_channel_name());
    }

    #[test]
    fn test_matches_hostmask_exact() {
        assert!(matches_hostmask("nick!user@host", "nick!user@host"));
        assert!(!matches_hostmask("nick!user@host", "other!user@host"));
    }

    #[test]
    fn test_matches_hostmask_wildcard_star() {
        assert!(matches_hostmask("*!*@*", "nick!user@host"));
        assert!(matches_hostmask("nick!*@*", "nick!user@host"));
        assert!(matches_hostmask("*!user@*", "nick!user@host"));
        assert!(matches_hostmask("*!*@host", "nick!user@host"));
        assert!(matches_hostmask(
            "*!*@*.example.com",
            "nick!user@sub.example.com"
        ));
    }

    #[test]
    fn test_matches_hostmask_wildcard_question() {
        assert!(matches_hostmask("nic?!user@host", "nick!user@host"));
        assert!(matches_hostmask("????!user@host", "nick!user@host"));
        assert!(!matches_hostmask("???!user@host", "nick!user@host"));
    }

    #[test]
    fn test_matches_hostmask_case_insensitive() {
        assert!(matches_hostmask("NICK!USER@HOST", "nick!user@host"));
        assert!(matches_hostmask("Nick!User@Host", "NICK!USER@HOST"));
    }
}
