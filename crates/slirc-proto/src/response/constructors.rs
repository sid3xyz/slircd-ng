//! Semantic error constructors for `Response`.
//!
//! This module provides static methods on `Response` to construct standard error messages
//! as defined in RFC 2812 and modern IRC specifications.

use crate::command::Command;
use crate::message::Message;
use crate::response::Response;

macro_rules! impl_err {
    (
        $(#[$meta:meta])*
        $name:ident, $resp:ident, $msg:literal
    ) => {
        $(#[$meta])*
        pub fn $name(client: &str) -> Message {
            Self::error_msg(
                Response::$resp,
                vec![client.to_string(), $msg.to_string()],
            )
        }
    };
    (
        $(#[$meta:meta])*
        $name:ident, $resp:ident, $arg:ident, $msg:literal
    ) => {
        $(#[$meta])*
        pub fn $name(client: &str, $arg: &str) -> Message {
            Self::error_msg(
                Response::$resp,
                vec![
                    client.to_string(),
                    $arg.to_string(),
                    $msg.to_string(),
                ],
            )
        }
    };
    (
        $(#[$meta:meta])*
        $name:ident, $resp:ident, fmt($arg:ident, $fmt:literal)
    ) => {
        $(#[$meta])*
        pub fn $name(client: &str, $arg: &str) -> Message {
            Self::error_msg(
                Response::$resp,
                vec![
                    client.to_string(),
                    format!($fmt, $arg),
                ],
            )
        }
    };
    (
        $(#[$meta:meta])*
        $name:ident, $resp:ident, $arg1:ident, $arg2:ident, $msg:literal
    ) => {
        $(#[$meta])*
        pub fn $name(client: &str, $arg1: &str, $arg2: &str) -> Message {
            Self::error_msg(
                Response::$resp,
                vec![
                    client.to_string(),
                    $arg1.to_string(),
                    $arg2.to_string(),
                    $msg.to_string(),
                ],
            )
        }
    };
    (
        $(#[$meta:meta])*
        $name:ident, $resp:ident, $arg1:ident, $arg2:ident
    ) => {
        $(#[$meta])*
        pub fn $name(client: &str, $arg1: &str, $arg2: &str) -> Message {
            Self::error_msg(
                Response::$resp,
                vec![
                    client.to_string(),
                    $arg1.to_string(),
                    $arg2.to_string(),
                ],
            )
        }
    };
}

impl Response {
    /// Helper to construct a Message with a Response command.
    fn error_msg(response: Response, args: Vec<String>) -> Message {
        Message {
            tags: None,
            prefix: None,
            command: Command::Response(response, args),
        }
    }

    /// `281 RPL_ACCEPTLIST`
    /// `<nick>`
    pub fn rpl_acceptlist(client: &str, nick: &str) -> Message {
        Self::error_msg(
            Response::RPL_ACCEPTLIST,
            vec![client.to_string(), nick.to_string()],
        )
    }

    /// `282 RPL_ENDOFACCEPT`
    /// `:End of /ACCEPT list`
    pub fn rpl_endofaccept(client: &str) -> Message {
        Self::error_msg(
            Response::RPL_ENDOFACCEPT,
            vec![client.to_string(), "End of /ACCEPT list".to_string()],
        )
    }

    // === 400-499 Error Replies ===

    impl_err!(
        /// `401 ERR_NOSUCHNICK`
        /// `<nickname> :No such nick/channel`
        err_nosuchnick, ERR_NOSUCHNICK, nickname, "No such nick/channel"
    );

    impl_err!(
        /// `403 ERR_NOSUCHCHANNEL`
        /// `<channel name> :No such channel`
        err_nosuchchannel, ERR_NOSUCHCHANNEL, channel, "No such channel"
    );

    impl_err!(
        /// `404 ERR_CANNOTSENDTOCHAN`
        /// `<channel name> :Cannot send to channel`
        err_cannotsendtochan, ERR_CANNOTSENDTOCHAN, channel, "Cannot send to channel"
    );

    impl_err!(
        /// `405 ERR_TOOMANYCHANNELS`
        /// `<channel name> :You have joined too many channels`
        err_toomanychannels, ERR_TOOMANYCHANNELS, channel, "You have joined too many channels"
    );

    impl_err!(
        /// `406 ERR_WASNOSUCHNICK`
        /// `<nickname> :There was no such nickname`
        err_wasnosuchnick, ERR_WASNOSUCHNICK, nickname, "There was no such nickname"
    );

    /// `407 ERR_TOOMANYTARGETS`
    /// `<target> :<error code> recipients. <abort message>`
    pub fn err_toomanytargets(
        client: &str,
        target: &str,
        error_code: &str,
        abort_msg: &str,
    ) -> Message {
        Self::error_msg(
            Response::ERR_TOOMANYTARGETS,
            vec![
                client.to_string(),
                target.to_string(),
                format!("{} recipients. {}", error_code, abort_msg),
            ],
        )
    }

    impl_err!(
        /// `409 ERR_NOORIGIN`
        /// `:No origin specified`
        err_noorigin, ERR_NOORIGIN, "No origin specified"
    );

    impl_err!(
        /// `410 ERR_INVALIDCAPCMD`
        /// `<subcommand> :Invalid CAP subcommand`
        err_invalidcapcmd, ERR_INVALIDCAPCMD, subcommand, "Invalid CAP subcommand"
    );

    impl_err!(
        /// `411 ERR_NORECIPIENT`
        /// `:No recipient given (<command>)`
        err_norecipient, ERR_NORECIPIENT, fmt(command, "No recipient given ({})")
    );

    impl_err!(
        /// `412 ERR_NOTEXTTOSEND`
        /// `:No text to send`
        err_notexttosend, ERR_NOTEXTTOSEND, "No text to send"
    );

    impl_err!(
        /// `413 ERR_NOTOPLEVEL`
        /// `<mask> :No toplevel domain specified`
        err_notoplevel, ERR_NOTOPLEVEL, mask, "No toplevel domain specified"
    );

    impl_err!(
        /// `414 ERR_WILDTOPLEVEL`
        /// `<mask> :Wildcard in toplevel domain`
        err_wildtoplevel, ERR_WILDTOPLEVEL, mask, "Wildcard in toplevel domain"
    );

    impl_err!(
        /// `415 ERR_BADMASK`
        /// `<mask> :Bad Server/host mask`
        err_badmask, ERR_BADMASK, mask, "Bad Server/host mask"
    );

    impl_err!(
        /// `421 ERR_UNKNOWNCOMMAND`
        /// `<command> :Unknown command`
        err_unknowncommand, ERR_UNKNOWNCOMMAND, command, "Unknown command"
    );

    /// `422 ERR_NOMOTD`
    /// `:MOTD File is missing`
    pub fn err_nomotd(client: &str) -> Message {
        Self::error_msg(
            Response::ERR_NOMOTD,
            vec![client.to_string(), "MOTD File is missing".to_string()],
        )
    }

    /// `423 ERR_NOADMININFO`
    /// `<server> :No administrative info available`
    pub fn err_noadmininfo(client: &str, server: &str) -> Message {
        Self::error_msg(
            Response::ERR_NOADMININFO,
            vec![
                client.to_string(),
                server.to_string(),
                "No administrative info available".to_string(),
            ],
        )
    }

    /// `424 ERR_FILEERROR`
    /// `:File error doing <file op> on <file>`
    pub fn err_fileerror(client: &str, op: &str, file: &str) -> Message {
        Self::error_msg(
            Response::ERR_FILEERROR,
            vec![
                client.to_string(),
                format!("File error doing {} on {}", op, file),
            ],
        )
    }

    impl_err!(
        /// `431 ERR_NONICKNAMEGIVEN`
        /// `:No nickname given`
        err_nonicknamegiven, ERR_NONICKNAMEGIVEN, "No nickname given"
    );

    impl_err!(
        /// `432 ERR_ERRONEUSNICKNAME`
        /// `<nick> :Erroneous nickname`
        err_erroneusnickname, ERR_ERRONEOUSNICKNAME, nick, "Erroneous nickname"
    );

    impl_err!(
        /// `433 ERR_NICKNAMEINUSE`
        /// `<nick> :Nickname is already in use`
        err_nicknameinuse, ERR_NICKNAMEINUSE, nick, "Nickname is already in use"
    );

    /// `436 ERR_NICKCOLLISION`
    /// `<nick> :Nickname collision KILL from <user>@<host>`
    pub fn err_nickcollision(client: &str, nick: &str, user: &str, host: &str) -> Message {
        Self::error_msg(
            Response::ERR_NICKCOLLISION,
            vec![
                client.to_string(),
                nick.to_string(),
                format!("Nickname collision KILL from {}@{}", user, host),
            ],
        )
    }

    impl_err!(
        /// `437 ERR_UNAVAILRESOURCE`
        /// `<nick/channel> :Nick/channel is temporarily unavailable`
        err_unavailresource, ERR_UNAVAILRESOURCE, resource, "Nick/channel is temporarily unavailable"
    );

    impl_err!(
        /// `441 ERR_USERNOTINCHANNEL`
        /// `<nick> <channel> :They aren't on that channel`
        err_usernotinchannel, ERR_USERNOTINCHANNEL, nick, channel, "They aren't on that channel"
    );

    impl_err!(
        /// `442 ERR_NOTONCHANNEL`
        /// `<channel> :You're not on that channel`
        err_notonchannel, ERR_NOTONCHANNEL, channel, "You're not on that channel"
    );

    impl_err!(
        /// `443 ERR_USERONCHANNEL`
        /// `<user> <channel> :is already on channel`
        err_useronchannel, ERR_USERONCHANNEL, user, channel, "is already on channel"
    );

    /// `444 ERR_NOLOGIN`
    /// `<user> :User not logged in`
    pub fn err_nologin(client: &str, user: &str) -> Message {
        Self::error_msg(
            Response::ERR_NOLOGIN,
            vec![
                client.to_string(),
                user.to_string(),
                "User not logged in".to_string(),
            ],
        )
    }

    /// `445 ERR_SUMMONDISABLED`
    /// `:SUMMON has been disabled`
    pub fn err_summondisabled(client: &str) -> Message {
        Self::error_msg(
            Response::ERR_SUMMONDISABLED,
            vec![client.to_string(), "SUMMON has been disabled".to_string()],
        )
    }

    /// `446 ERR_USERSDISABLED`
    /// `:USERS has been disabled`
    pub fn err_usersdisabled(client: &str) -> Message {
        Self::error_msg(
            Response::ERR_USERSDISABLED,
            vec![client.to_string(), "USERS has been disabled".to_string()],
        )
    }

    /// `447 ERR_NONICKCHANGE`
    /// `<nickname> :Can't change nickname while on <channel> (+N)`
    pub fn err_nonickchange(client: &str, nickname: &str, channel: &str) -> Message {
        Self::error_msg(
            Response::ERR_NONICKCHANGE,
            vec![
                client.to_string(),
                nickname.to_string(),
                format!("Can't change nickname while on {} (+N)", channel),
            ],
        )
    }

    impl_err!(
        /// `451 ERR_NOTREGISTERED`
        /// `:You have not registered`
        err_notregistered, ERR_NOTREGISTERED, "You have not registered"
    );

    impl_err!(
        /// `456 ERR_ACCEPTFULL`
        /// `:Accept list is full`
        err_accept_full, ERR_ACCEPTFULL, "Accept list is full"
    );

    impl_err!(
        /// `457 ERR_ACCEPTEXIST`
        /// `<nick> :is already on your accept list`
        err_accept_exist, ERR_ACCEPTEXIST, nick, "is already on your accept list"
    );

    impl_err!(
        /// `458 ERR_ACCEPTNOT`
        /// `<nick> :is not on your accept list`
        err_accept_not, ERR_ACCEPTNOT, nick, "is not on your accept list"
    );

    impl_err!(
        /// `461 ERR_NEEDMOREPARAMS`
        /// `<command> :Not enough parameters`
        err_needmoreparams, ERR_NEEDMOREPARAMS, command, "Not enough parameters"
    );

    impl_err!(
        /// `462 ERR_ALREADYREGISTRED`
        /// `:Unauthorized command (already registered)`
        err_alreadyregistred, ERR_ALREADYREGISTERED, "Unauthorized command (already registered)"
    );

    impl_err!(
        /// `463 ERR_NOPERMFORHOST`
        /// `:Your host isn't among the privileged`
        err_nopermforhost, ERR_NOPERMFORHOST, "Your host isn't among the privileged"
    );

    impl_err!(
        /// `464 ERR_PASSWDMISMATCH`
        /// `:Password incorrect`
        err_passwdmismatch, ERR_PASSWDMISMATCH, "Password incorrect"
    );

    impl_err!(
        /// `465 ERR_YOUREBANNEDCREEP`
        /// `:You are banned from this server`
        err_yourebannedcreep, ERR_YOUREBANNEDCREEP, "You are banned from this server"
    );

    impl_err!(
        /// `477 ERR_NEEDREGGEDNICK`
        /// `<target> :You need to be a registered nick to send to that target`
        err_needreggednick, ERR_NEEDREGGEDNICK, target, "You need to be a registered nick to send to that target"
    );

    impl_err!(
        /// `466 ERR_YOUWILLBEBANNED`
        err_youwillbebanned, ERR_YOUWILLBEBANNED, "You will be banned"
    );

    impl_err!(
        /// `467 ERR_KEYSET`
        /// `<channel> :Channel key already set`
        err_keyset, ERR_KEYSET, channel, "Channel key already set"
    );

    /// `471 ERR_CHANNELISFULL`
    /// `<channel> :Cannot join channel (+l)`
    pub fn err_channelisfull(client: &str, channel: &str) -> Message {
        Self::error_msg(
            Response::ERR_CHANNELISFULL,
            vec![
                client.to_string(),
                channel.to_string(),
                "Cannot join channel (+l)".to_string(),
            ],
        )
    }

    /// `472 ERR_UNKNOWNMODE`
    /// `<char> :is unknown mode char to me for <channel>`
    pub fn err_unknownmode(client: &str, mode_char: char, channel: &str) -> Message {
        Self::error_msg(
            Response::ERR_UNKNOWNMODE,
            vec![
                client.to_string(),
                mode_char.to_string(),
                format!("is unknown mode char to me for {}", channel),
            ],
        )
    }

    impl_err!(
        /// `473 ERR_INVITEONLYCHAN`
        /// `<channel> :Cannot join channel (+i)`
        err_inviteonlychan, ERR_INVITEONLYCHAN, channel, "Cannot join channel (+i)"
    );

    impl_err!(
        /// `474 ERR_BANNEDFROMCHAN`
        /// `<channel> :Cannot join channel (+b)`
        err_bannedfromchan, ERR_BANNEDFROMCHAN, channel, "Cannot join channel (+b)"
    );

    impl_err!(
        /// `475 ERR_BADCHANNELKEY`
        /// `<channel> :Cannot join channel (+k)`
        err_badchannelkey, ERR_BADCHANNELKEY, channel, "Cannot join channel (+k)"
    );

    impl_err!(
        /// `476 ERR_BADCHANMASK`
        /// `<channel> :Bad Channel Mask`
        err_badchanmask, ERR_BADCHANMASK, channel, "Bad Channel Mask"
    );

    impl_err!(
        /// `477 ERR_NEEDREGGEDNICK`
        /// `<channel/nick> :You need to be registered to send to this target`
        err_need_regged_nick, ERR_NEEDREGGEDNICK, target, "You need to be registered to send to this target"
    );

    /// `478 ERR_BANLISTFULL`
    /// `<channel> <char> :Channel list is full`
    pub fn err_banlistfull(client: &str, channel: &str, mode_char: char) -> Message {
        Self::error_msg(
            Response::ERR_BANLISTFULL,
            vec![
                client.to_string(),
                channel.to_string(),
                mode_char.to_string(),
                "Channel list is full".to_string(),
            ],
        )
    }

    impl_err!(
        /// `481 ERR_NOPRIVILEGES`
        /// `:Permission Denied- You're not an IRC operator`
        err_noprivileges, ERR_NOPRIVILEGES, "Permission Denied- You're not an IRC operator"
    );

    impl_err!(
        /// `482 ERR_CHANOPRIVSNEEDED`
        /// `<channel> :You're not channel operator`
        err_chanoprivsneeded, ERR_CHANOPRIVSNEEDED, channel, "You're not channel operator"
    );

    impl_err!(
        /// `483 ERR_CANTKILLSERVER`
        /// `:You can't kill a server!`
        err_cantkillserver, ERR_CANTKILLSERVER, "You can't kill a server!"
    );

    impl_err!(
        /// `484 ERR_RESTRICTED`
        /// `:Your connection is restricted!`
        err_restricted, ERR_RESTRICTED, "Your connection is restricted!"
    );

    /// `485 ERR_UNIQOPPRIVSNEEDED`
    /// `:You're not the original channel operator`
    pub fn err_uniqopprivsneeded(client: &str) -> Message {
        Self::error_msg(
            Response::ERR_UNIQOPPRIVSNEEDED,
            vec![
                client.to_string(),
                "You're not the original channel operator".to_string(),
            ],
        )
    }

    /// `491 ERR_NOOPERHOST`
    /// `:No O-lines for your host`
    pub fn err_nooperhost(client: &str) -> Message {
        Self::error_msg(
            Response::ERR_NOOPERHOST,
            vec![client.to_string(), "No O-lines for your host".to_string()],
        )
    }

    /// `501 ERR_UMODEUNKNOWNFLAG`
    /// `:Unknown MODE flag`
    pub fn err_umodeunknownflag(client: &str) -> Message {
        Self::error_msg(
            Response::ERR_UMODEUNKNOWNFLAG,
            vec![client.to_string(), "Unknown MODE flag".to_string()],
        )
    }

    /// `502 ERR_USERSDONTMATCH`
    /// `:Cannot change mode for other users`
    pub fn err_usersdontmatch(client: &str) -> Message {
        Self::error_msg(
            Response::ERR_USERSDONTMATCH,
            vec![
                client.to_string(),
                "Cannot change mode for other users".to_string(),
            ],
        )
    }

    // === 700-799 Help/Monitor Replies ===

    /// `704 RPL_HELPSTART`
    /// `<subject> :Start of HELP`
    pub fn rpl_helpstart(client: &str, subject: &str) -> Message {
        Self::error_msg(
            Response::RPL_HELPSTART,
            vec![
                client.to_string(),
                subject.to_string(),
                "Start of HELP".to_string(),
            ],
        )
    }

    impl_err!(
        /// `705 RPL_HELPTXT`
        /// `<subject> :<text>`
        rpl_helptxt, RPL_HELPTXT, subject, text
    );

    impl_err!(
        /// `706 RPL_ENDOFHELP`
        /// `<subject> :End of HELP`
        rpl_endofhelp, RPL_ENDOFHELP, subject, "End of HELP"
    );

    impl_err!(
        /// `524 ERR_HELPNOTFOUND`
        /// `<subject> :No help available on this topic`
        err_helpnotfound, ERR_HELPNOTFOUND, subject, "No help available on this topic"
    );

    // === 900-999 SASL Replies ===

    /// `900 RPL_LOGGEDIN`
    /// `<nick>!<user>@<host> <account> :You are now logged in as <account>`
    pub fn rpl_loggedin(client: &str, mask: &str, account: &str) -> Message {
        Self::error_msg(
            Response::RPL_LOGGEDIN,
            vec![
                client.to_string(),
                mask.to_string(),
                account.to_string(),
                format!("You are now logged in as {}", account),
            ],
        )
    }

    impl_err!(
        /// `903 RPL_SASLSUCCESS`
        /// `:SASL authentication successful`
        rpl_saslsuccess, RPL_SASLSUCCESS, "SASL authentication successful"
    );

    impl_err!(
        /// `904 ERR_SASLFAIL`
        /// `:SASL authentication failed`
        err_saslfail, ERR_SASLFAIL, "SASL authentication failed"
    );

    // === 670 / 691 STARTTLS Replies (IRCv3) ===

    /// `670 RPL_STARTTLS`
    /// `:STARTTLS successful, proceed with TLS handshake`
    pub fn rpl_starttls(client: &str) -> Message {
        Self::error_msg(
            Response::RPL_STARTTLS,
            vec![
                client.to_string(),
                "STARTTLS successful, proceed with TLS handshake".to_string(),
            ],
        )
    }

    /// `691 ERR_STARTTLS`
    /// `<reason>`
    pub fn err_starttls(client: &str, reason: &str) -> Message {
        Self::error_msg(
            Response::ERR_STARTTLS,
            vec![client.to_string(), reason.to_string()],
        )
    }
}
