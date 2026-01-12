//! IRC command parsing implementation.

mod channel;
mod connection;
mod ircv3;
mod messaging;
mod server;
mod user;

use super::types::Command;
use crate::chan::ChannelExt;
use crate::error::MessageParseError;
use crate::mode::Mode;

/// Parse a MODE command, dispatching to channel or user mode parsing.
fn parse_mode_command(original_cmd: &str, args: Vec<&str>) -> Result<Command, MessageParseError> {
    if args.is_empty() {
        return Ok(connection::raw(original_cmd, args));
    }

    let target = args[0];
    let mode_args = &args[1..];

    if target.is_channel_name() {
        Ok(Command::ChannelMODE(
            target.to_owned(),
            Mode::as_channel_modes(mode_args)?,
        ))
    } else {
        Ok(Command::UserMODE(
            target.to_owned(),
            Mode::as_user_modes(mode_args)?,
        ))
    }
}

impl Command {
    /// Parse a command from its name and arguments.
    #[must_use = "command parsing result should be handled"]
    pub fn new(cmd: &str, args: Vec<&str>) -> Result<Command, MessageParseError> {
        let cmd_upper = cmd.to_ascii_uppercase();
        let cmd_str = cmd_upper.as_str();

        match cmd_str {
            "PASS" | "NICK" | "USER" | "OPER" | "SERVICE" | "QUIT" | "SQUIT" => {
                connection::parse(cmd_str, args)
            }

            "JOIN" | "PART" | "TOPIC" | "NAMES" | "LIST" | "INVITE" | "KICK" => {
                channel::parse(cmd_str, args)
            }

            "MOTD" | "LUSERS" | "VERSION" | "STATS" | "LINKS" | "TIME" | "CONNECT" | "TRACE"
            | "ADMIN" | "INFO" | "MAP" | "RULES" | "USERIP" | "HELP" | "METADATA" | "SERVLIST"
            | "SQUERY" | "SERVER" | "SID" | "UID" | "SJOIN" | "TMODE" | "ENCAP" | "CAPAB"
            | "SVINFO" => server::parse(cmd_str, args),

            "WHO" | "WHOIS" | "WHOWAS" => user::parse(cmd_str, args),

            "PRIVMSG" | "NOTICE" | "PING" | "PONG" | "ERROR" | "AWAY" | "REHASH" | "DIE"
            | "RESTART" | "SUMMON" | "USERS" | "WALLOPS" | "GLOBOPS" | "USERHOST" | "ISON"
            | "KILL" | "SAJOIN" | "SAMODE" | "SANICK" | "SAPART" | "SAQUIT" | "NICKSERV"
            | "CHANSERV" | "OPERSERV" | "BOTSERV" | "HOSTSERV" | "MEMOSERV" | "NS" | "CS"
            | "OS" | "BS" | "HS" | "MS" | "KLINE" | "DLINE" | "UNKLINE" | "UNDLINE" | "GLINE"
            | "UNGLINE" | "ZLINE" | "UNZLINE" | "RLINE" | "UNRLINE" | "SHUN" | "UNSHUN"
            | "KNOCK" | "ACCEPT" | "NPC" | "RELAYMSG" => messaging::parse(cmd_str, args),

            "CAP" | "AUTHENTICATE" | "ACCOUNT" | "BATCH" | "CHGHOST" | "CHGIDENT" | "SETNAME"
            | "MONITOR" | "TAGMSG" | "WEBIRC" | "CHATHISTORY" | "ACK" => {
                ircv3::parse(cmd_str, args)
            }

            "MODE" => parse_mode_command(cmd, args),

            _ => {
                if let Ok(resp) = cmd.parse() {
                    Ok(Command::Response(
                        resp,
                        args.into_iter().map(|s| s.to_owned()).collect(),
                    ))
                } else {
                    Ok(connection::raw(cmd, args))
                }
            }
        }
    }
}
