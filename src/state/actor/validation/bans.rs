//! Ban and exception matching for channel access control.
//!
//! Provides utilities to check if a user is banned from a channel,
//! with support for extended bans and exception lists.

use crate::security::{UserContext, matches_ban_or_except};
use crate::state::ListEntry;

/// Create IRC user mask (nick!user@host).
pub fn format_user_mask(nick: &str, user: &str, host: &str) -> String {
    format!("{}!{}@{}", nick, user, host)
}

/// Create IRC user mask from a UserContext.
pub fn create_user_mask(user_context: &UserContext) -> String {
    format_user_mask(
        &user_context.nickname,
        &user_context.username,
        &user_context.hostname,
    )
}

/// Check if a user is banned, accounting for exceptions.
pub fn is_banned(
    user_mask: &str,
    user_context: &UserContext,
    bans: &[ListEntry],
    excepts: &[ListEntry],
) -> bool {
    for ban in bans {
        if matches_ban_or_except(&ban.mask, user_mask, user_context) {
            let is_excepted = excepts
                .iter()
                .any(|e| matches_ban_or_except(&e.mask, user_mask, user_context));

            if !is_excepted {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_user_mask_basic() {
        assert_eq!(
            format_user_mask("nick", "user", "host"),
            "nick!user@host"
        );
    }

    #[test]
    fn test_format_user_mask_with_numbers() {
        assert_eq!(
            format_user_mask("User123", "ident456", "192.168.1.1"),
            "User123!ident456@192.168.1.1"
        );
    }

    #[test]
    fn test_format_user_mask_with_special_chars() {
        assert_eq!(
            format_user_mask("nick[away]", "~user", "irc.example.com"),
            "nick[away]!~user@irc.example.com"
        );
    }

    #[test]
    fn test_format_user_mask_empty_components() {
        // Edge case: empty strings should still format correctly
        assert_eq!(format_user_mask("", "", ""), "!@");
        assert_eq!(format_user_mask("nick", "", ""), "nick!@");
        assert_eq!(format_user_mask("", "user", ""), "!user@");
        assert_eq!(format_user_mask("", "", "host"), "!@host");
    }

    #[test]
    fn test_format_user_mask_with_wildcards() {
        // Wildcards are valid in masks
        assert_eq!(
            format_user_mask("*", "*", "*"),
            "*!*@*"
        );
        assert_eq!(
            format_user_mask("nick*", "*user", "*.example.com"),
            "nick*!*user@*.example.com"
        );
    }

    #[test]
    fn test_format_user_mask_unicode() {
        // Unicode should be preserved as-is
        assert_eq!(
            format_user_mask("Ñick", "üser", "hõst.com"),
            "Ñick!üser@hõst.com"
        );
    }

    #[test]
    fn test_format_user_mask_long_components() {
        let long_nick = "a".repeat(50);
        let long_user = "b".repeat(50);
        let long_host = "c".repeat(100);
        let expected = format!("{}!{}@{}", long_nick, long_user, long_host);
        assert_eq!(format_user_mask(&long_nick, &long_user, &long_host), expected);
    }

    #[test]
    fn test_create_user_mask_basic() {
        let ctx = UserContext {
            nickname: "TestNick".to_string(),
            username: "testuser".to_string(),
            hostname: "test.example.com".to_string(),
            realname: "Test User".to_string(),
            account: None,
            server: "irc.example.com".to_string(),
            channels: vec![],
            is_oper: false,
            oper_type: None,
            certificate_fp: None,
            sasl_mechanism: None,
            is_registered: false,
            is_tls: false,
        };
        assert_eq!(create_user_mask(&ctx), "TestNick!testuser@test.example.com");
    }

    #[test]
    fn test_create_user_mask_with_account() {
        let ctx = UserContext {
            nickname: "AuthedUser".to_string(),
            username: "authed".to_string(),
            hostname: "irc.network.org".to_string(),
            realname: "Authenticated User".to_string(),
            account: Some("myaccount".to_string()),
            server: "server.network.org".to_string(),
            channels: vec!["#test".to_string()],
            is_oper: true,
            oper_type: Some("admin".to_string()),
            certificate_fp: None,
            sasl_mechanism: Some("PLAIN".to_string()),
            is_registered: true,
            is_tls: true,
        };
        assert_eq!(create_user_mask(&ctx), "AuthedUser!authed@irc.network.org");
    }

    #[test]
    fn test_create_user_mask_with_tilde() {
        let ctx = UserContext {
            nickname: "Guest".to_string(),
            username: "~guest".to_string(),
            hostname: "unverified.host.net".to_string(),
            realname: "Guest User".to_string(),
            account: None,
            server: "server.host.net".to_string(),
            channels: vec![],
            is_oper: false,
            oper_type: None,
            certificate_fp: None,
            sasl_mechanism: None,
            is_registered: false,
            is_tls: false,
        };
        assert_eq!(create_user_mask(&ctx), "Guest!~guest@unverified.host.net");
    }
}
