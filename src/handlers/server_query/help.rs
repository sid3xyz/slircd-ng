//! HELP command handler.
//!
//! Returns help text for IRC commands.
//! RFC 2812 doesn't define HELP, but it's a common modern extension.

use super::super::{
    Context, Handler, HandlerError, HandlerResult,
};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Static help text for commands.
const HELP_TOPICS: &[(&str, &[&str])] = &[
    (
        "ADMIN",
        &[
            "ADMIN [server]",
            "Returns administrative info about the server.",
        ],
    ),
    (
        "AWAY",
        &[
            "AWAY [message]",
            "Marks you as away. Without message, clears away status.",
        ],
    ),
    (
        "CAP",
        &[
            "CAP LS|LIST|REQ|END [args]",
            "IRCv3 capability negotiation.",
        ],
    ),
    (
        "CHANSERV",
        &[
            "CHANSERV <command> [args]",
            "Send command to ChanServ. Alias: CS",
        ],
    ),
    (
        "DIE",
        &["DIE", "Shuts down the server (IRC operators only)."],
    ),
    (
        "HELP",
        &[
            "HELP [command]",
            "Shows help for a command, or lists all commands.",
        ],
    ),
    (
        "INFO",
        &["INFO [server]", "Returns information about the server."],
    ),
    (
        "INVITE",
        &["INVITE <nick> <channel>", "Invites a user to a channel."],
    ),
    (
        "ISON",
        &["ISON <nick> [nick...]", "Checks if users are online."],
    ),
    (
        "JOIN",
        &[
            "JOIN <channel>[,channel...] [key[,key...]]",
            "Joins one or more channels.",
        ],
    ),
    (
        "KICK",
        &[
            "KICK <channel> <nick> [reason]",
            "Kicks a user from a channel.",
        ],
    ),
    (
        "KILL",
        &[
            "KILL <nick> <reason>",
            "Disconnects a user (IRC operators only).",
        ],
    ),
    (
        "KNOCK",
        &[
            "KNOCK <channel> [message]",
            "Requests an invite to a channel.",
        ],
    ),
    (
        "LINKS",
        &["LINKS [[server] mask]", "Lists servers matching the mask."],
    ),
    (
        "LIST",
        &["LIST [channel[,channel...]] [server]", "Lists channels."],
    ),
    (
        "LUSERS",
        &["LUSERS [mask [server]]", "Returns user statistics."],
    ),
    (
        "MODE",
        &[
            "MODE <target> [modes [args]]",
            "Sets or queries modes on channels/users.",
        ],
    ),
    (
        "MONITOR",
        &[
            "MONITOR +|-|C|L|S [nick[,nick...]]",
            "Online status notifications.",
        ],
    ),
    (
        "MOTD",
        &["MOTD [server]", "Returns the Message of the Day."],
    ),
    (
        "NAMES",
        &[
            "NAMES [channel[,channel...]] [server]",
            "Lists users in channels.",
        ],
    ),
    ("NICK", &["NICK <nickname>", "Changes your nickname."]),
    (
        "NICKSERV",
        &[
            "NICKSERV <command> [args]",
            "Send command to NickServ. Alias: NS",
        ],
    ),
    (
        "NOTICE",
        &[
            "NOTICE <target> <message>",
            "Sends a notice (no auto-reply).",
        ],
    ),
    (
        "OPER",
        &[
            "OPER <name> <password>",
            "Authenticates as an IRC operator.",
        ],
    ),
    (
        "PART",
        &[
            "PART <channel>[,channel...] [message]",
            "Leaves one or more channels.",
        ],
    ),
    ("PING", &["PING <server>", "Tests connection to server."]),
    ("PONG", &["PONG <server>", "Replies to a PING."]),
    (
        "PRIVMSG",
        &[
            "PRIVMSG <target> <message>",
            "Sends a message to a user or channel.",
        ],
    ),
    ("QUIT", &["QUIT [message]", "Disconnects from the server."]),
    (
        "REHASH",
        &[
            "REHASH",
            "Reloads server configuration (IRC operators only).",
        ],
    ),
    (
        "RESTART",
        &["RESTART", "Restarts the server (IRC operators only)."],
    ),
    (
        "SETNAME",
        &["SETNAME <realname>", "Changes your realname (IRCv3)."],
    ),
    (
        "STATS",
        &["STATS [query [server]]", "Returns server statistics."],
    ),
    (
        "TIME",
        &["TIME [server]", "Returns the server's local time."],
    ),
    (
        "TOPIC",
        &[
            "TOPIC <channel> [topic]",
            "Sets or queries a channel's topic.",
        ],
    ),
    (
        "USERHOST",
        &[
            "USERHOST <nick> [nick...]",
            "Returns hostmask info for users.",
        ],
    ),
    (
        "VERSION",
        &["VERSION [server]", "Returns the server version."],
    ),
    (
        "WALLOPS",
        &["WALLOPS <message>", "Sends message to all operators."],
    ),
    ("WHO", &["WHO [mask [o]]", "Lists users matching the mask."]),
    (
        "WHOIS",
        &[
            "WHOIS [server] <nick>[,nick...]",
            "Returns information about users.",
        ],
    ),
    (
        "WHOWAS",
        &[
            "WHOWAS <nick> [count [server]]",
            "Returns info about disconnected users.",
        ],
    ),
];

/// Handler for HELP command.
///
/// `HELP [subject]`
///
/// Returns help on a specific command or lists all commands.
pub struct HelpHandler;

#[async_trait]
impl Handler for HelpHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        let subject = msg.arg(0);

        match subject {
            Some(topic) => {
                let topic_upper = topic.to_ascii_uppercase();
                if let Some((cmd, lines)) = HELP_TOPICS.iter().find(|(c, _)| *c == topic_upper) {
                    // RPL_HELPSTART (704)
                    ctx.send_reply(
                        Response::RPL_HELPSTART,
                        vec![nick.clone(), cmd.to_string(), lines[0].to_string()],
                    )
                    .await?;

                    // RPL_HELPTXT (705) for additional lines
                    for line in &lines[1..] {
                        ctx.send_reply(
                            Response::RPL_HELPTXT,
                            vec![nick.clone(), cmd.to_string(), line.to_string()],
                        )
                        .await?;
                    }

                    // RPL_ENDOFHELP (706)
                    ctx.send_reply(
                        Response::RPL_ENDOFHELP,
                        vec![nick.clone(), cmd.to_string(), "End of /HELP".to_string()],
                    )
                    .await?;
                } else {
                    // ERR_HELPNOTFOUND (524)
                    ctx.send_reply(
                        Response::ERR_HELPNOTFOUND,
                        vec![
                            nick.clone(),
                            topic.to_string(),
                            "No help available on that topic".to_string(),
                        ],
                    )
                    .await?;
                }
            }
            None => {
                // List all commands
                ctx.send_reply(
                    Response::RPL_HELPSTART,
                    vec![
                        nick.clone(),
                        "index".to_string(),
                        "Available commands:".to_string(),
                    ],
                )
                .await?;

                // Group commands into lines of ~10 each
                let commands: Vec<&str> = HELP_TOPICS.iter().map(|(c, _)| *c).collect();
                for chunk in commands.chunks(10) {
                    let line = chunk.join(" ");
                    ctx.send_reply(
                        Response::RPL_HELPTXT,
                        vec![nick.clone(), "index".to_string(), line],
                    )
                    .await?;
                }

                ctx.send_reply(
                    Response::RPL_HELPTXT,
                    vec![nick.clone(), "index".to_string(), " ".to_string()],
                )
                .await?;

                ctx.send_reply(
                    Response::RPL_HELPTXT,
                    vec![
                        nick.clone(),
                        "index".to_string(),
                        "Use /HELP <command> for help on a specific command.".to_string(),
                    ],
                )
                .await?;

                ctx.send_reply(
                    Response::RPL_ENDOFHELP,
                    vec![
                        nick.clone(),
                        "index".to_string(),
                        "End of /HELP".to_string(),
                    ],
                )
                .await?;
            }
        }

        Ok(())
    }
}
