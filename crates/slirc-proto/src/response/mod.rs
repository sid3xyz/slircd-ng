//! IRC numeric response codes as defined in RFC 2812 and modern IRC specifications.
//!
//! This module provides an enumeration of IRC server response codes (numerics).
//! Response codes are three-digit numbers sent by servers to indicate the result
//! of commands or to provide information.
//!
//! # Reference
//! - RFC 2812: Internet Relay Chat: Client Protocol
//! - Modern IRC documentation: <https://modern.ircdocs.horse/>

#![allow(non_camel_case_types)]

mod constructors;
mod errors;
mod helpers;
mod numerics;

// Re-export error type
pub use helpers::ParseResponseError;

/// IRC server response code.
///
/// Response codes are categorized as:
/// - 001-099: Connection/registration
/// - 200-399: Command replies
/// - 400-599: Error replies
/// - 600-999: Extended/modern numerics
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u16)]
#[non_exhaustive]
pub enum Response {
    // === Connection Registration (001-099) ===
    /// 001 - Welcome to the IRC network
    RPL_WELCOME = 1,
    /// 002 - Your host is running version
    RPL_YOURHOST = 2,
    /// 003 - Server creation date
    RPL_CREATED = 3,
    /// 004 - Server info (name, version, user modes, channel modes)
    RPL_MYINFO = 4,
    /// 005 - Server supported features (ISUPPORT)
    RPL_ISUPPORT = 5,
    /// 010 - Bounce to another server
    RPL_BOUNCE = 10,
    /// 042 - Your unique ID
    RPL_YOURID = 42,

    // === Command Responses (200-399) ===

    // Trace replies
    /// 200 - Trace link
    RPL_TRACELINK = 200,
    /// 201 - Trace connecting
    RPL_TRACECONNECTING = 201,
    /// 202 - Trace handshake
    RPL_TRACEHANDSHAKE = 202,
    /// 203 - Trace unknown
    RPL_TRACEUNKNOWN = 203,
    /// 204 - Trace operator
    RPL_TRACEOPERATOR = 204,
    /// 205 - Trace user
    RPL_TRACEUSER = 205,
    /// 206 - Trace server
    RPL_TRACESERVER = 206,
    /// 207 - Trace service
    RPL_TRACESERVICE = 207,
    /// 208 - Trace new type
    RPL_TRACENEWTYPE = 208,
    /// 209 - Trace class
    RPL_TRACECLASS = 209,
    /// 210 - Trace reconnect
    RPL_TRACERECONNECT = 210,

    // Stats replies
    /// 211 - Stats link info
    RPL_STATSLINKINFO = 211,
    /// 212 - Stats commands
    RPL_STATSCOMMANDS = 212,
    /// 216 - Stats K-line
    RPL_STATSKLINE = 216,
    /// 219 - End of stats
    RPL_ENDOFSTATS = 219,
    /// 220 - Stats D-line
    RPL_STATSDLINE = 220,
    /// 221 - User mode string
    RPL_UMODEIS = 221,
    /// 226 - Stats shun
    RPL_STATSSHUN = 226,
    /// 234 - Service list
    RPL_SERVLIST = 234,
    /// 235 - Service list end
    RPL_SERVLISTEND = 235,
    /// 242 - Stats uptime
    RPL_STATSUPTIME = 242,
    /// 243 - Stats O-line
    RPL_STATSOLINE = 243,
    /// 249 - Stats debug/custom
    RPL_STATSDEBUG = 249,

    // ACCEPT (Caller ID)
    /// 281 - Accept list entry
    RPL_ACCEPTLIST = 281,
    /// 282 - End of accept list
    RPL_ENDOFACCEPT = 282,

    // Luser replies
    /// 251 - Luser client count
    RPL_LUSERCLIENT = 251,
    /// 252 - Luser operator count
    RPL_LUSEROP = 252,
    /// 253 - Luser unknown connections
    RPL_LUSERUNKNOWN = 253,
    /// 254 - Luser channel count
    RPL_LUSERCHANNELS = 254,
    /// 255 - Luser local info
    RPL_LUSERME = 255,

    // Admin replies
    /// 256 - Admin info start
    RPL_ADMINME = 256,
    /// 257 - Admin location 1
    RPL_ADMINLOC1 = 257,
    /// 258 - Admin location 2
    RPL_ADMINLOC2 = 258,
    /// 259 - Admin email
    RPL_ADMINEMAIL = 259,

    // Trace/stats end
    /// 261 - Trace log
    RPL_TRACELOG = 261,
    /// 262 - Trace end
    RPL_TRACEEND = 262,
    /// 263 - Try again later
    RPL_TRYAGAIN = 263,

    // Local/global users
    /// 265 - Local users
    RPL_LOCALUSERS = 265,
    /// 266 - Global users
    RPL_GLOBALUSERS = 266,

    // Silence list
    /// 271 - Silence list entry
    RPL_SILELIST = 271,
    /// 272 - End of silence list
    RPL_ENDOFSILELIST = 272,

    /// 276 - WHOIS certificate fingerprint
    RPL_WHOISCERTFP = 276,

    // Misc
    /// 300 - None (dummy placeholder)
    RPL_NONE = 300,
    /// 301 - User is away
    RPL_AWAY = 301,
    /// 302 - USERHOST reply
    RPL_USERHOST = 302,
    /// 303 - ISON reply
    RPL_ISON = 303,
    /// 305 - You are no longer marked as away
    RPL_UNAWAY = 305,
    /// 306 - You have been marked as away
    RPL_NOWAWAY = 306,

    // WHOIS replies
    /// 311 - WHOIS user info
    RPL_WHOISUSER = 311,
    /// 312 - WHOIS server
    RPL_WHOISSERVER = 312,
    /// 313 - WHOIS operator status
    RPL_WHOISOPERATOR = 313,
    /// 314 - WHOWAS user info
    RPL_WHOWASUSER = 314,
    /// 315 - End of WHO
    RPL_ENDOFWHO = 315,
    /// 317 - WHOIS idle time
    RPL_WHOISIDLE = 317,
    /// 318 - End of WHOIS
    RPL_ENDOFWHOIS = 318,
    /// 319 - WHOIS channels
    RPL_WHOISCHANNELS = 319,

    // Channel/list replies
    /// 321 - List start
    RPL_LISTSTART = 321,
    /// 322 - List entry
    RPL_LIST = 322,
    /// 323 - List end
    RPL_LISTEND = 323,
    /// 324 - Channel mode
    RPL_CHANNELMODEIS = 324,
    /// 325 - Channel unique operator
    RPL_UNIQOPIS = 325,
    /// 329 - Channel creation time
    RPL_CREATIONTIME = 329,
    /// 330 - WHOIS account name
    RPL_WHOISACCOUNT = 330,
    /// 331 - No topic set
    RPL_NOTOPIC = 331,
    /// 332 - Channel topic
    RPL_TOPIC = 332,
    /// 333 - Topic set by/time
    RPL_TOPICWHOTIME = 333,
    /// 335 - WHOIS bot flag
    RPL_WHOISBOT = 335,
    /// 338 - WHOIS actually (real host)
    RPL_WHOISACTUALLY = 338,
    /// 340 - USERIP reply
    RPL_USERIP = 340,
    /// 341 - Inviting user to channel
    RPL_INVITING = 341,
    /// 342 - Summoning user
    RPL_SUMMONING = 342,
    /// 346 - Invite list entry
    RPL_INVITELIST = 346,
    /// 347 - End of invite list
    RPL_ENDOFINVITELIST = 347,
    /// 348 - Exception list entry
    RPL_EXCEPTLIST = 348,
    /// 349 - End of exception list
    RPL_ENDOFEXCEPTLIST = 349,
    /// 351 - Server version
    RPL_VERSION = 351,
    /// 352 - WHO reply
    RPL_WHOREPLY = 352,
    /// 353 - NAMES reply
    RPL_NAMREPLY = 353,
    /// 354 - WHOX reply
    RPL_WHOSPCRPL = 354,

    // Links/info
    /// 364 - Links entry
    RPL_LINKS = 364,
    /// 365 - End of links
    RPL_ENDOFLINKS = 365,
    /// 366 - End of NAMES
    RPL_ENDOFNAMES = 366,
    /// 367 - Ban list entry
    RPL_BANLIST = 367,
    /// 368 - End of ban list
    RPL_ENDOFBANLIST = 368,
    /// 369 - End of WHOWAS
    RPL_ENDOFWHOWAS = 369,
    /// 371 - Info text
    RPL_INFO = 371,
    /// 372 - MOTD text
    RPL_MOTD = 372,
    /// 374 - End of info
    RPL_ENDOFINFO = 374,
    /// 375 - MOTD start
    RPL_MOTDSTART = 375,
    /// 376 - End of MOTD
    RPL_ENDOFMOTD = 376,
    /// 378 - WHOIS host
    RPL_WHOISHOST = 378,
    /// 379 - WHOIS modes
    RPL_WHOISMODES = 379,

    // Oper/rehash
    /// 381 - You are now an operator
    RPL_YOUREOPER = 381,
    /// 382 - Rehashing config
    RPL_REHASHING = 382,
    /// 383 - You are a service
    RPL_YOURESERVICE = 383,
    /// 391 - Server time
    RPL_TIME = 391,
    /// 392 - Users start
    RPL_USERSSTART = 392,
    /// 393 - Users entry
    RPL_USERS = 393,
    /// 394 - End of users
    RPL_ENDOFUSERS = 394,
    /// 395 - No users
    RPL_NOUSERS = 395,
    /// 396 - Host hidden
    RPL_HOSTHIDDEN = 396,

    // === Error Replies (400-599) ===
    /// 400 - Unknown error
    ERR_UNKNOWNERROR = 400,
    /// 401 - No such nick
    ERR_NOSUCHNICK = 401,
    /// 402 - No such server
    ERR_NOSUCHSERVER = 402,
    /// 403 - No such channel
    ERR_NOSUCHCHANNEL = 403,
    /// 404 - Cannot send to channel
    ERR_CANNOTSENDTOCHAN = 404,
    /// 405 - Too many channels
    ERR_TOOMANYCHANNELS = 405,
    /// 406 - Was no such nick
    ERR_WASNOSUCHNICK = 406,
    /// 407 - Too many targets
    ERR_TOOMANYTARGETS = 407,
    /// 408 - No such service
    ERR_NOSUCHSERVICE = 408,
    /// 409 - No origin
    ERR_NOORIGIN = 409,
    /// 410 - Invalid CAP command
    ERR_INVALIDCAPCMD = 410,
    /// 411 - No recipient
    ERR_NORECIPIENT = 411,
    /// 412 - No text to send
    ERR_NOTEXTTOSEND = 412,
    /// 413 - No top level domain
    ERR_NOTOPLEVEL = 413,
    /// 414 - Wildcard in top level
    ERR_WILDTOPLEVEL = 414,
    /// 415 - Bad mask
    ERR_BADMASK = 415,
    /// 417 - Input too long
    ERR_INPUTTOOLONG = 417,
    /// 421 - Unknown command
    ERR_UNKNOWNCOMMAND = 421,
    /// 422 - No MOTD
    ERR_NOMOTD = 422,
    /// 423 - No admin info
    ERR_NOADMININFO = 423,
    /// 424 - File error
    ERR_FILEERROR = 424,
    /// 431 - No nickname given
    ERR_NONICKNAMEGIVEN = 431,
    /// 432 - Erroneous nickname
    ERR_ERRONEOUSNICKNAME = 432,
    /// 433 - Nickname in use
    ERR_NICKNAMEINUSE = 433,
    /// 436 - Nick collision
    ERR_NICKCOLLISION = 436,
    /// 437 - Resource unavailable
    ERR_UNAVAILRESOURCE = 437,
    /// 441 - User not in channel
    ERR_USERNOTINCHANNEL = 441,
    /// 442 - Not on channel
    ERR_NOTONCHANNEL = 442,
    /// 443 - User on channel
    ERR_USERONCHANNEL = 443,
    /// 444 - No login
    ERR_NOLOGIN = 444,
    /// 445 - Summon disabled
    ERR_SUMMONDISABLED = 445,
    /// 446 - Users disabled
    ERR_USERSDISABLED = 446,
    /// 447 - Cannot change nick while in +N channel
    ERR_NONICKCHANGE = 447,
    /// 451 - Not registered
    ERR_NOTREGISTERED = 451,
    /// 456 - Accept list full
    ERR_ACCEPTFULL = 456,
    /// 457 - Accept list exists
    ERR_ACCEPTEXIST = 457,
    /// 458 - Accept list not found
    ERR_ACCEPTNOT = 458,
    /// 461 - Need more params
    ERR_NEEDMOREPARAMS = 461,
    /// 462 - Already registered
    ERR_ALREADYREGISTERED = 462,
    /// 463 - No permission for host
    ERR_NOPERMFORHOST = 463,
    /// 464 - Password mismatch
    ERR_PASSWDMISMATCH = 464,
    /// 465 - You are banned
    ERR_YOUREBANNEDCREEP = 465,
    /// 466 - You will be banned
    ERR_YOUWILLBEBANNED = 466,
    /// 467 - Key already set
    ERR_KEYSET = 467,
    /// 471 - Channel is full
    ERR_CHANNELISFULL = 471,
    /// 472 - Unknown mode
    ERR_UNKNOWNMODE = 472,
    /// 473 - Invite only channel
    ERR_INVITEONLYCHAN = 473,
    /// 474 - Banned from channel
    ERR_BANNEDFROMCHAN = 474,
    /// 475 - Bad channel key
    ERR_BADCHANNELKEY = 475,
    /// 476 - Bad channel mask
    ERR_BADCHANMASK = 476,
    /// 477 - Need registered nick (or channel doesn't support modes)
    ERR_NEEDREGGEDNICK = 477,
    /// 478 - Ban list full
    ERR_BANLISTFULL = 478,
    /// 479 - Bad channel name
    ERR_BADCHANNAME = 479,
    /// 481 - No privileges
    ERR_NOPRIVILEGES = 481,
    /// 482 - Channel op privileges needed
    ERR_CHANOPRIVSNEEDED = 482,
    /// 483 - Cannot kill server
    ERR_CANTKILLSERVER = 483,
    /// 484 - Restricted
    ERR_RESTRICTED = 484,
    /// 485 - Unique op privileges needed
    ERR_UNIQOPPRIVSNEEDED = 485,
    /// 489 - Secure only channel
    ERR_SECUREONLYCHAN = 489,
    /// 491 - No oper host
    ERR_NOOPERHOST = 491,
    /// 520 - Oper only channel (InspIRCd extension)
    ERR_OPERONLY = 520,
    /// 501 - Unknown mode flag
    ERR_UMODEUNKNOWNFLAG = 501,
    /// 502 - Users don't match
    ERR_USERSDONTMATCH = 502,
    /// 511 - Silence list full
    ERR_SILELISTFULL = 511,
    /// 524 - Help not found
    ERR_HELPNOTFOUND = 524,
    /// 525 - Invalid channel key
    ERR_INVALIDKEY = 525,
    /// 573 - Cannot send roleplay message (Ergo extension)
    ERR_CANNOTSENDRP = 573,

    // === Extended/Modern Numerics (600+) ===
    /// 606 - Map entry
    RPL_MAP = 606,
    /// 607 - End of map
    RPL_MAPEND = 607,
    /// 632 - Rules start
    RPL_RULESTART = 632,
    /// 633 - Rules text
    RPL_RULES = 633,
    /// 634 - End of rules
    RPL_ENDOFRULES = 634,
    /// 635 - No rules
    ERR_NORULES = 635,
    /// 646 - Stats P-line
    RPL_STATSPLINE = 646,
    /// 671 - WHOIS secure connection
    RPL_WHOISSECURE = 671,

    // === STARTTLS (670, 691) ===
    /// 670 - STARTTLS successful
    RPL_STARTTLS = 670,
    /// 691 - STARTTLS failed
    ERR_STARTTLS = 691,

    /// 696 - Invalid mode parameter
    ERR_INVALIDMODEPARAM = 696,
    /// 704 - Help start
    RPL_HELPSTART = 704,
    /// 705 - Help text
    RPL_HELPTXT = 705,
    /// 706 - End of help
    RPL_ENDOFHELP = 706,
    /// 710 - Knock
    RPL_KNOCK = 710,
    /// 711 - Knock delivered
    RPL_KNOCKDLVR = 711,
    /// 712 - Too many knocks
    ERR_TOOMANYKNOCK = 712,
    /// 713 - Channel open
    ERR_CHANOPEN = 713,
    /// 714 - Knock on channel
    ERR_KNOCKONCHAN = 714,
    /// 723 - No privileges
    ERR_NOPRIVS = 723,
    /// 728 - Quiet list entry
    RPL_QUIETLIST = 728,
    /// 729 - End of quiet list
    RPL_ENDOFQUIETLIST = 729,

    // Monitor
    /// 730 - Monitor online
    RPL_MONONLINE = 730,
    /// 731 - Monitor offline
    RPL_MONOFFLINE = 731,
    /// 732 - Monitor list
    RPL_MONLIST = 732,
    /// 733 - End of monitor list
    RPL_ENDOFMONLIST = 733,
    /// 734 - Monitor list full
    ERR_MONLISTFULL = 734,

    // Metadata
    /// 760 - WHOIS key/value
    RPL_WHOISKEYVALUE = 760,
    /// 761 - Key/value
    RPL_KEYVALUE = 761,
    /// 762 - End of metadata
    RPL_METADATAEND = 762,
    /// 764 - Metadata limit exceeded
    ERR_METADATALIMIT = 764,
    /// 765 - Target invalid
    ERR_TARGETINVALID = 765,
    /// 766 - No matching key
    ERR_NOMATCHINGKEY = 766,
    /// 767 - Key invalid
    ERR_KEYINVALID = 767,
    /// 768 - Key not set
    ERR_KEYNOTSET = 768,
    /// 769 - Key no permission
    ERR_KEYNOPERMISSION = 769,

    // SASL (IRCv3)
    /// 900 - Logged in
    RPL_LOGGEDIN = 900,
    /// 901 - Logged out
    RPL_LOGGEDOUT = 901,
    /// 902 - Nick locked
    ERR_NICKLOCKED = 902,
    /// 903 - SASL success
    RPL_SASLSUCCESS = 903,
    /// 904 - SASL fail
    ERR_SASLFAIL = 904,
    /// 905 - SASL too long
    ERR_SASLTOOLONG = 905,
    /// 906 - SASL aborted
    ERR_SASLABORT = 906,
    /// 907 - SASL already authenticated
    ERR_SASLALREADY = 907,
    /// 908 - SASL mechanisms
    RPL_SASLMECHS = 908,
}

/// Deprecated alias for [`Response::ERR_ALREADYREGISTERED`].
///
/// The original RFC 1459/2812 used the typo'd spelling "ALREADYREGISTRED".
/// Modern IRC documentation uses the correct spelling "ALREADYREGISTERED".
#[deprecated(since = "1.1.0", note = "use ERR_ALREADYREGISTERED (correct spelling)")]
pub const ERR_ALREADYREGISTRED: Response = Response::ERR_ALREADYREGISTERED;

impl Response {
    /// Deprecated alias for [`Response::ERR_NEEDREGGEDNICK`].
    #[deprecated(since = "1.3.0", note = "use ERR_NEEDREGGEDNICK")]
    pub const ERR_NOCHANMODES: Response = Response::ERR_NEEDREGGEDNICK;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_code() {
        assert_eq!(Response::RPL_WELCOME.code(), 1);
        assert_eq!(Response::ERR_NICKNAMEINUSE.code(), 433);
        assert_eq!(Response::RPL_ENDOFMOTD.code(), 376);
    }

    #[test]
    fn test_from_code() {
        assert_eq!(Response::from_code(1), Some(Response::RPL_WELCOME));
        assert_eq!(Response::from_code(433), Some(Response::ERR_NICKNAMEINUSE));
        assert_eq!(Response::from_code(9999), None);
    }

    #[test]
    fn test_is_error() {
        assert!(!Response::RPL_WELCOME.is_error());
        assert!(Response::ERR_NICKNAMEINUSE.is_error());
        assert!(Response::ERR_NOSUCHNICK.is_error());
    }

    #[test]
    fn test_parse() {
        assert_eq!("001".parse::<Response>().unwrap(), Response::RPL_WELCOME);
        assert_eq!(
            "433".parse::<Response>().unwrap(),
            Response::ERR_NICKNAMEINUSE
        );
        assert!("abc".parse::<Response>().is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Response::RPL_WELCOME), "001");
        assert_eq!(format!("{}", Response::ERR_NICKNAMEINUSE), "433");
    }

    #[test]
    fn test_is_reply() {
        assert!(Response::RPL_AWAY.is_reply());
        assert!(Response::RPL_TOPIC.is_reply());
        assert!(!Response::RPL_WELCOME.is_reply());
        assert!(!Response::ERR_NOSUCHNICK.is_reply());
    }

    #[test]
    fn test_is_sasl() {
        assert!(Response::RPL_LOGGEDIN.is_sasl());
        assert!(Response::RPL_SASLSUCCESS.is_sasl());
        assert!(Response::ERR_SASLFAIL.is_sasl());
        assert!(!Response::RPL_WELCOME.is_sasl());
    }

    #[test]
    fn test_is_channel_related() {
        assert!(Response::RPL_TOPIC.is_channel_related());
        assert!(Response::RPL_NAMREPLY.is_channel_related());
        assert!(Response::ERR_NOSUCHCHANNEL.is_channel_related());
        assert!(!Response::RPL_WELCOME.is_channel_related());
    }

    #[test]
    fn test_is_whois_related() {
        assert!(Response::RPL_WHOISUSER.is_whois_related());
        assert!(Response::RPL_ENDOFWHOIS.is_whois_related());
        assert!(!Response::RPL_WELCOME.is_whois_related());
    }

    #[test]
    fn test_category() {
        assert_eq!(Response::RPL_WELCOME.category(), "Connection Registration");
        assert_eq!(
            Response::RPL_TRACELINK.category(),
            "Command Replies (Trace/Stats)"
        );
        assert_eq!(
            Response::RPL_TOPIC.category(),
            "Command Replies (User/Channel)"
        );
        assert_eq!(Response::ERR_NOSUCHNICK.category(), "Error Replies");
        assert_eq!(Response::RPL_SASLSUCCESS.category(), "SASL/Account");
    }
}
