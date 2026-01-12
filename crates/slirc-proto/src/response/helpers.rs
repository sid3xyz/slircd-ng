//! Helper methods and trait implementations for IRC response codes.
//!
//! This module provides utility methods for Response enum including:
//! - Code conversion (from_code, code)
//! - Type checking (is_error, is_reply, etc.)
//! - Category classification
//! - Display/parsing traits

use super::Response;
use std::str::FromStr;

impl Response {
    /// Returns the numeric code as u16
    #[inline]
    pub fn code(&self) -> u16 {
        *self as u16
    }

    /// Creates a Response from a numeric code
    pub fn from_code(code: u16) -> Option<Response> {
        // Try success codes first
        if let Some(resp) = Response::from_success_code(code) {
            return Some(resp);
        }
        // Try error codes
        if let Some(resp) = Response::from_error_code(code) {
            return Some(resp);
        }
        None
    }

    /// Check if this is an error response (4xx, 5xx, or specific error codes)
    #[inline]
    pub fn is_error(&self) -> bool {
        let code = self.code();
        (400..600).contains(&code)
            || code == 723
            || code == 734
            || (765..=769).contains(&code)
            || code == 902
            || (904..=907).contains(&code)
    }

    /// Check if this is a success/informational response
    #[inline]
    pub fn is_success(&self) -> bool {
        !self.is_error()
    }

    /// Check if this is a connection registration response (001-099)
    #[inline]
    pub fn is_registration(&self) -> bool {
        self.code() < 100
    }

    /// Check if this is a command reply (200-399)
    #[inline]
    pub fn is_reply(&self) -> bool {
        let code = self.code();
        (200..400).contains(&code)
    }

    /// Check if this is a SASL-related response (900-908)
    #[inline]
    pub fn is_sasl(&self) -> bool {
        let code = self.code();
        (900..=908).contains(&code)
    }

    /// Check if this is a channel-related response
    #[inline]
    pub fn is_channel_related(&self) -> bool {
        matches!(
            self,
            Response::RPL_TOPIC
                | Response::RPL_NOTOPIC
                | Response::RPL_TOPICWHOTIME
                | Response::RPL_NAMREPLY
                | Response::RPL_ENDOFNAMES
                | Response::RPL_CHANNELMODEIS
                | Response::RPL_CREATIONTIME
                | Response::RPL_BANLIST
                | Response::RPL_ENDOFBANLIST
                | Response::RPL_EXCEPTLIST
                | Response::RPL_ENDOFEXCEPTLIST
                | Response::RPL_INVITELIST
                | Response::RPL_ENDOFINVITELIST
                | Response::RPL_QUIETLIST
                | Response::RPL_ENDOFQUIETLIST
                | Response::ERR_NOSUCHCHANNEL
                | Response::ERR_CANNOTSENDTOCHAN
                | Response::ERR_TOOMANYCHANNELS
                | Response::ERR_CHANNELISFULL
                | Response::ERR_INVITEONLYCHAN
                | Response::ERR_BANNEDFROMCHAN
                | Response::ERR_BADCHANNELKEY
                | Response::ERR_BADCHANMASK
                | Response::ERR_BADCHANNAME
                | Response::ERR_CHANOPRIVSNEEDED
                | Response::ERR_NOTONCHANNEL
                | Response::ERR_USERNOTINCHANNEL
                | Response::ERR_USERONCHANNEL
                | Response::ERR_NEEDREGGEDNICK
                | Response::ERR_BANLISTFULL
                | Response::ERR_SECUREONLYCHAN
        )
    }

    /// Check if this is a WHOIS/WHOWAS-related response
    #[inline]
    pub fn is_whois_related(&self) -> bool {
        matches!(
            self,
            Response::RPL_WHOISUSER
                | Response::RPL_WHOISSERVER
                | Response::RPL_WHOISOPERATOR
                | Response::RPL_WHOISIDLE
                | Response::RPL_ENDOFWHOIS
                | Response::RPL_WHOISCHANNELS
                | Response::RPL_WHOISACCOUNT
                | Response::RPL_WHOISBOT
                | Response::RPL_WHOISACTUALLY
                | Response::RPL_WHOISHOST
                | Response::RPL_WHOISMODES
                | Response::RPL_WHOISCERTFP
                | Response::RPL_WHOISSECURE
                | Response::RPL_WHOISKEYVALUE
                | Response::RPL_WHOWASUSER
                | Response::RPL_ENDOFWHOWAS
        )
    }

    /// Returns the RFC 2812 category name for this response
    pub fn category(&self) -> &'static str {
        let code = self.code();
        match code {
            1..=99 => "Connection Registration",
            200..=299 => "Command Replies (Trace/Stats)",
            300..=399 => "Command Replies (User/Channel)",
            400..=499 => "Error Replies",
            500..=599 => "Error Replies (Server)",
            600..=699 => "Extended Replies",
            700..=799 => "Extended Replies (IRCv3)",
            900..=999 => "SASL/Account",
            _ => "Unknown",
        }
    }
}

impl FromStr for Response {
    type Err = ParseResponseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let code: u16 = s.parse().map_err(|_| ParseResponseError::InvalidFormat)?;
        Response::from_code(code).ok_or(ParseResponseError::UnknownCode(code))
    }
}

impl std::fmt::Display for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:03}", self.code())
    }
}

/// Error when parsing a response code
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseResponseError {
    /// The string was not a valid number
    InvalidFormat,
    /// The numeric code is not a known response
    UnknownCode(u16),
}

impl std::fmt::Display for ParseResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "invalid response code format"),
            Self::UnknownCode(code) => write!(f, "unknown response code: {}", code),
        }
    }
}

impl std::error::Error for ParseResponseError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // code() tests
    // ============================================================

    #[test]
    fn code_returns_correct_numeric_value() {
        assert_eq!(Response::RPL_WELCOME.code(), 1);
        assert_eq!(Response::RPL_YOURHOST.code(), 2);
        assert_eq!(Response::RPL_CREATED.code(), 3);
        assert_eq!(Response::RPL_MYINFO.code(), 4);
        assert_eq!(Response::RPL_ISUPPORT.code(), 5);
    }

    #[test]
    fn code_returns_correct_error_codes() {
        assert_eq!(Response::ERR_NOSUCHNICK.code(), 401);
        assert_eq!(Response::ERR_NOSUCHCHANNEL.code(), 403);
        assert_eq!(Response::ERR_UNKNOWNCOMMAND.code(), 421);
        assert_eq!(Response::ERR_NICKNAMEINUSE.code(), 433);
    }

    #[test]
    fn code_returns_correct_reply_codes() {
        assert_eq!(Response::RPL_LUSERCLIENT.code(), 251);
        assert_eq!(Response::RPL_LUSEROP.code(), 252);
        assert_eq!(Response::RPL_TOPIC.code(), 332);
        assert_eq!(Response::RPL_NAMREPLY.code(), 353);
    }

    // ============================================================
    // from_code() tests
    // ============================================================

    #[test]
    fn from_code_returns_some_for_known_codes() {
        assert_eq!(Response::from_code(1), Some(Response::RPL_WELCOME));
        assert_eq!(Response::from_code(2), Some(Response::RPL_YOURHOST));
        assert_eq!(Response::from_code(5), Some(Response::RPL_ISUPPORT));
        assert_eq!(Response::from_code(401), Some(Response::ERR_NOSUCHNICK));
        assert_eq!(Response::from_code(433), Some(Response::ERR_NICKNAMEINUSE));
    }

    #[test]
    fn from_code_returns_none_for_unknown_codes() {
        assert_eq!(Response::from_code(9999), None);
        assert_eq!(Response::from_code(0), None);
        assert_eq!(Response::from_code(65535), None);
    }

    #[test]
    fn from_code_roundtrips_with_code() {
        // For any Response, from_code(r.code()) should return Some(r)
        let responses = [
            Response::RPL_WELCOME,
            Response::RPL_YOURHOST,
            Response::ERR_NOSUCHNICK,
            Response::ERR_NOSUCHCHANNEL,
            Response::RPL_TOPIC,
            Response::RPL_WHOISUSER,
        ];
        for r in responses {
            assert_eq!(Response::from_code(r.code()), Some(r));
        }
    }

    // ============================================================
    // is_error() tests
    // ============================================================

    #[test]
    fn is_error_true_for_4xx_codes() {
        assert!(Response::ERR_NOSUCHNICK.is_error()); // 401
        assert!(Response::ERR_NOSUCHCHANNEL.is_error()); // 403
        assert!(Response::ERR_UNKNOWNCOMMAND.is_error()); // 421
        assert!(Response::ERR_NICKNAMEINUSE.is_error()); // 433
        assert!(Response::ERR_NOTONCHANNEL.is_error()); // 442
    }

    #[test]
    fn is_error_true_for_5xx_codes() {
        assert!(Response::ERR_NOPRIVILEGES.is_error()); // 481
        assert!(Response::ERR_CHANOPRIVSNEEDED.is_error()); // 482
    }

    #[test]
    fn is_error_true_for_special_error_codes() {
        // 723 - ERR_NOPRIVS
        assert!(Response::ERR_NOPRIVS.is_error());
        // 902-907 are SASL error codes
        assert!(Response::ERR_NICKLOCKED.is_error()); // 902
        assert!(Response::ERR_SASLFAIL.is_error()); // 904
        assert!(Response::ERR_SASLTOOLONG.is_error()); // 905
        assert!(Response::ERR_SASLABORT.is_error()); // 906
        assert!(Response::ERR_SASLALREADY.is_error()); // 907
    }

    #[test]
    fn is_error_false_for_success_codes() {
        assert!(!Response::RPL_WELCOME.is_error()); // 001
        assert!(!Response::RPL_YOURHOST.is_error()); // 002
        assert!(!Response::RPL_TOPIC.is_error()); // 332
        assert!(!Response::RPL_NAMREPLY.is_error()); // 353
    }

    #[test]
    fn is_error_false_for_sasl_success() {
        assert!(!Response::RPL_LOGGEDIN.is_error()); // 900
        assert!(!Response::RPL_LOGGEDOUT.is_error()); // 901
        assert!(!Response::RPL_SASLSUCCESS.is_error()); // 903
        assert!(!Response::RPL_SASLMECHS.is_error()); // 908
    }

    // ============================================================
    // is_success() tests
    // ============================================================

    #[test]
    fn is_success_inverse_of_is_error() {
        let responses = [
            Response::RPL_WELCOME,
            Response::ERR_NOSUCHNICK,
            Response::RPL_TOPIC,
            Response::ERR_NICKNAMEINUSE,
            Response::RPL_SASLSUCCESS,
            Response::ERR_SASLFAIL,
        ];
        for r in responses {
            assert_eq!(r.is_success(), !r.is_error());
        }
    }

    #[test]
    fn is_success_true_for_welcome_sequence() {
        assert!(Response::RPL_WELCOME.is_success());
        assert!(Response::RPL_YOURHOST.is_success());
        assert!(Response::RPL_CREATED.is_success());
        assert!(Response::RPL_MYINFO.is_success());
        assert!(Response::RPL_ISUPPORT.is_success());
    }

    // ============================================================
    // is_registration() tests
    // ============================================================

    #[test]
    fn is_registration_true_for_sub_100_codes() {
        assert!(Response::RPL_WELCOME.is_registration()); // 001
        assert!(Response::RPL_YOURHOST.is_registration()); // 002
        assert!(Response::RPL_CREATED.is_registration()); // 003
        assert!(Response::RPL_MYINFO.is_registration()); // 004
        assert!(Response::RPL_ISUPPORT.is_registration()); // 005
    }

    #[test]
    fn is_registration_false_for_100_plus_codes() {
        assert!(!Response::RPL_LUSERCLIENT.is_registration()); // 251
        assert!(!Response::RPL_TOPIC.is_registration()); // 332
        assert!(!Response::ERR_NOSUCHNICK.is_registration()); // 401
    }

    // ============================================================
    // is_reply() tests
    // ============================================================

    #[test]
    fn is_reply_true_for_200_to_399_codes() {
        assert!(Response::RPL_TRACELINK.is_reply()); // 200
        assert!(Response::RPL_LUSERCLIENT.is_reply()); // 251
        assert!(Response::RPL_TOPIC.is_reply()); // 332
        assert!(Response::RPL_NAMREPLY.is_reply()); // 353
    }

    #[test]
    fn is_reply_false_for_codes_outside_200_399() {
        assert!(!Response::RPL_WELCOME.is_reply()); // 001
        assert!(!Response::RPL_ISUPPORT.is_reply()); // 005
        assert!(!Response::ERR_NOSUCHNICK.is_reply()); // 401
        assert!(!Response::ERR_NICKNAMEINUSE.is_reply()); // 433
    }

    // ============================================================
    // is_sasl() tests
    // ============================================================

    #[test]
    fn is_sasl_true_for_900_to_908_codes() {
        assert!(Response::RPL_LOGGEDIN.is_sasl()); // 900
        assert!(Response::RPL_LOGGEDOUT.is_sasl()); // 901
        assert!(Response::ERR_NICKLOCKED.is_sasl()); // 902
        assert!(Response::RPL_SASLSUCCESS.is_sasl()); // 903
        assert!(Response::ERR_SASLFAIL.is_sasl()); // 904
        assert!(Response::ERR_SASLTOOLONG.is_sasl()); // 905
        assert!(Response::ERR_SASLABORT.is_sasl()); // 906
        assert!(Response::ERR_SASLALREADY.is_sasl()); // 907
        assert!(Response::RPL_SASLMECHS.is_sasl()); // 908
    }

    #[test]
    fn is_sasl_false_for_non_sasl_codes() {
        assert!(!Response::RPL_WELCOME.is_sasl()); // 001
        assert!(!Response::ERR_NOSUCHNICK.is_sasl()); // 401
        assert!(!Response::RPL_TOPIC.is_sasl()); // 332
    }

    // ============================================================
    // is_channel_related() tests
    // ============================================================

    #[test]
    fn is_channel_related_true_for_channel_replies() {
        assert!(Response::RPL_TOPIC.is_channel_related());
        assert!(Response::RPL_NOTOPIC.is_channel_related());
        assert!(Response::RPL_TOPICWHOTIME.is_channel_related());
        assert!(Response::RPL_NAMREPLY.is_channel_related());
        assert!(Response::RPL_ENDOFNAMES.is_channel_related());
        assert!(Response::RPL_CHANNELMODEIS.is_channel_related());
        assert!(Response::RPL_BANLIST.is_channel_related());
        assert!(Response::RPL_ENDOFBANLIST.is_channel_related());
    }

    #[test]
    fn is_channel_related_true_for_channel_errors() {
        assert!(Response::ERR_NOSUCHCHANNEL.is_channel_related());
        assert!(Response::ERR_CANNOTSENDTOCHAN.is_channel_related());
        assert!(Response::ERR_TOOMANYCHANNELS.is_channel_related());
        assert!(Response::ERR_CHANNELISFULL.is_channel_related());
        assert!(Response::ERR_INVITEONLYCHAN.is_channel_related());
        assert!(Response::ERR_BANNEDFROMCHAN.is_channel_related());
        assert!(Response::ERR_BADCHANNELKEY.is_channel_related());
        assert!(Response::ERR_CHANOPRIVSNEEDED.is_channel_related());
        assert!(Response::ERR_NOTONCHANNEL.is_channel_related());
    }

    #[test]
    fn is_channel_related_false_for_non_channel_responses() {
        assert!(!Response::RPL_WELCOME.is_channel_related());
        assert!(!Response::RPL_YOURHOST.is_channel_related());
        assert!(!Response::ERR_NOSUCHNICK.is_channel_related());
        assert!(!Response::RPL_WHOISUSER.is_channel_related());
    }

    // ============================================================
    // is_whois_related() tests
    // ============================================================

    #[test]
    fn is_whois_related_true_for_whois_replies() {
        assert!(Response::RPL_WHOISUSER.is_whois_related());
        assert!(Response::RPL_WHOISSERVER.is_whois_related());
        assert!(Response::RPL_WHOISOPERATOR.is_whois_related());
        assert!(Response::RPL_WHOISIDLE.is_whois_related());
        assert!(Response::RPL_ENDOFWHOIS.is_whois_related());
        assert!(Response::RPL_WHOISCHANNELS.is_whois_related());
        assert!(Response::RPL_WHOISACCOUNT.is_whois_related());
    }

    #[test]
    fn is_whois_related_true_for_whowas_replies() {
        assert!(Response::RPL_WHOWASUSER.is_whois_related());
        assert!(Response::RPL_ENDOFWHOWAS.is_whois_related());
    }

    #[test]
    fn is_whois_related_false_for_non_whois_responses() {
        assert!(!Response::RPL_WELCOME.is_whois_related());
        assert!(!Response::RPL_TOPIC.is_whois_related());
        assert!(!Response::ERR_NOSUCHNICK.is_whois_related());
        assert!(!Response::RPL_NAMREPLY.is_whois_related());
    }

    // ============================================================
    // category() tests
    // ============================================================

    #[test]
    fn category_connection_registration() {
        assert_eq!(Response::RPL_WELCOME.category(), "Connection Registration");
        assert_eq!(Response::RPL_YOURHOST.category(), "Connection Registration");
        assert_eq!(Response::RPL_ISUPPORT.category(), "Connection Registration");
    }

    #[test]
    fn category_command_replies_trace_stats() {
        assert_eq!(
            Response::RPL_TRACELINK.category(),
            "Command Replies (Trace/Stats)"
        );
        assert_eq!(
            Response::RPL_LUSERCLIENT.category(),
            "Command Replies (Trace/Stats)"
        );
    }

    #[test]
    fn category_command_replies_user_channel() {
        assert_eq!(
            Response::RPL_TOPIC.category(),
            "Command Replies (User/Channel)"
        );
        assert_eq!(
            Response::RPL_NAMREPLY.category(),
            "Command Replies (User/Channel)"
        );
        assert_eq!(
            Response::RPL_WHOISUSER.category(),
            "Command Replies (User/Channel)"
        );
    }

    #[test]
    fn category_error_replies() {
        assert_eq!(Response::ERR_NOSUCHNICK.category(), "Error Replies");
        assert_eq!(Response::ERR_NOSUCHCHANNEL.category(), "Error Replies");
        assert_eq!(Response::ERR_NICKNAMEINUSE.category(), "Error Replies");
    }

    #[test]
    fn category_sasl_account() {
        assert_eq!(Response::RPL_LOGGEDIN.category(), "SASL/Account");
        assert_eq!(Response::RPL_SASLSUCCESS.category(), "SASL/Account");
        assert_eq!(Response::ERR_SASLFAIL.category(), "SASL/Account");
    }

    // ============================================================
    // FromStr tests
    // ============================================================

    #[test]
    fn from_str_parses_valid_codes() {
        assert_eq!("001".parse::<Response>().unwrap(), Response::RPL_WELCOME);
        assert_eq!("1".parse::<Response>().unwrap(), Response::RPL_WELCOME);
        assert_eq!("401".parse::<Response>().unwrap(), Response::ERR_NOSUCHNICK);
        assert_eq!(
            "433".parse::<Response>().unwrap(),
            Response::ERR_NICKNAMEINUSE
        );
    }

    #[test]
    fn from_str_error_for_invalid_format() {
        assert_eq!(
            "abc".parse::<Response>().unwrap_err(),
            ParseResponseError::InvalidFormat
        );
        assert_eq!(
            "".parse::<Response>().unwrap_err(),
            ParseResponseError::InvalidFormat
        );
    }

    #[test]
    fn from_str_error_for_unknown_code() {
        assert_eq!(
            "9999".parse::<Response>().unwrap_err(),
            ParseResponseError::UnknownCode(9999)
        );
    }

    // ============================================================
    // Display tests
    // ============================================================

    #[test]
    fn display_formats_with_leading_zeros() {
        assert_eq!(format!("{}", Response::RPL_WELCOME), "001");
        assert_eq!(format!("{}", Response::RPL_YOURHOST), "002");
        assert_eq!(format!("{}", Response::RPL_ISUPPORT), "005");
    }

    #[test]
    fn display_formats_three_digit_codes() {
        assert_eq!(format!("{}", Response::ERR_NOSUCHNICK), "401");
        assert_eq!(format!("{}", Response::ERR_NICKNAMEINUSE), "433");
        assert_eq!(format!("{}", Response::RPL_TOPIC), "332");
    }

    // ============================================================
    // ParseResponseError tests
    // ============================================================

    #[test]
    fn parse_response_error_display() {
        assert_eq!(
            ParseResponseError::InvalidFormat.to_string(),
            "invalid response code format"
        );
        assert_eq!(
            ParseResponseError::UnknownCode(9999).to_string(),
            "unknown response code: 9999"
        );
    }

    #[test]
    fn parse_response_error_is_error_trait() {
        // Ensure it implements std::error::Error
        fn assert_error<E: std::error::Error>(_: &E) {}
        assert_error(&ParseResponseError::InvalidFormat);
        assert_error(&ParseResponseError::UnknownCode(0));
    }
}
