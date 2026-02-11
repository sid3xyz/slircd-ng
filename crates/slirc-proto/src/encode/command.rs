//! Command encoding implementation.

use std::io::{self, Write};

use super::IrcEncode;
use crate::command::util::{
    needs_colon_prefix, write_args_with_trailing, write_cmd, write_cmd_freeform,
    write_collapsed_mode_flags, write_service_args, write_standard_reply, IoWriteSink, IrcSink,
};
use crate::command::Command;

impl IrcEncode for Command {
    fn encode<W: Write>(&self, w: &mut W) -> io::Result<usize> {
        use crate::command::subcommands::ChatHistorySubCommand;
        let mut sink = IoWriteSink(w);
        let w = &mut sink;

        match self {
            Command::PASS(p) => write_cmd(w, "PASS", &[p]),
            Command::PassTs6 { password, sid } => {
                write_cmd_freeform(w, "PASS", &[password, "TS", "6", sid])
            }
            Command::NICK(n) => write_cmd(w, "NICK", &[n]),
            Command::USER(u, m, r) => write_cmd_freeform(w, "USER", &[u, m, "*", r]),
            Command::OPER(u, p) => write_cmd(w, "OPER", &[u, p]),
            Command::UserMODE(u, modes) => {
                let mut written = w.write_str("MODE ")?;
                written += w.write_str(u)?;
                if !modes.is_empty() {
                    written += w.write_char(' ')?;
                    written += write_collapsed_mode_flags(w, modes)?;
                }
                Ok(written)
            }
            Command::SERVICE(nick, r0, dist, typ, r1, info) => {
                write_cmd_freeform(w, "SERVICE", &[nick, r0, dist, typ, r1, info])
            }
            Command::QUIT(Some(m)) => write_cmd(w, "QUIT", &[m]),
            Command::QUIT(None) => w.write_str("QUIT"),
            Command::SQUIT(s, c) => write_cmd_freeform(w, "SQUIT", &[s, c]),

            // Channel Operations
            Command::JOIN(c, Some(k), Some(n)) => write_cmd(w, "JOIN", &[c, k, n]),
            Command::JOIN(c, Some(k), None) => write_cmd(w, "JOIN", &[c, k]),
            Command::JOIN(c, None, Some(n)) => write_cmd(w, "JOIN", &[c, n]),
            Command::JOIN(c, None, None) => write_cmd(w, "JOIN", &[c]),
            Command::PART(c, Some(m)) => write_cmd_freeform(w, "PART", &[c, m]),
            Command::PART(c, None) => write_cmd(w, "PART", &[c]),
            Command::ChannelMODE(c, modes) => {
                let mut written = w.write_str("MODE ")?;
                written += w.write_str(c)?;
                if !modes.is_empty() {
                    written += w.write_char(' ')?;
                    written += write_collapsed_mode_flags(w, modes)?;
                    let mode_args: Vec<_> = modes.iter().filter_map(|m| m.arg()).collect();
                    for (i, arg) in mode_args.iter().enumerate() {
                        written += w.write_char(' ')?;
                        // Last argument needs colon prefix if it contains space, is empty, or starts with ':'
                        let is_last = i == mode_args.len() - 1;
                        if is_last && needs_colon_prefix(arg) {
                            written += w.write_char(':')?;
                        }
                        written += w.write_str(arg)?;
                    }
                }
                Ok(written)
            }
            Command::TOPIC(c, Some(t)) => write_cmd_freeform(w, "TOPIC", &[c, t]),
            Command::TOPIC(c, None) => write_cmd(w, "TOPIC", &[c]),
            Command::NAMES(Some(c), Some(t)) => write_cmd(w, "NAMES", &[c, t]),
            Command::NAMES(Some(c), None) => write_cmd(w, "NAMES", &[c]),
            Command::NAMES(None, _) => w.write_str("NAMES"),
            Command::LIST(Some(c), Some(t)) => write_cmd(w, "LIST", &[c, t]),
            Command::LIST(Some(c), None) => write_cmd(w, "LIST", &[c]),
            Command::LIST(None, _) => w.write_str("LIST"),
            Command::INVITE(n, c) => write_cmd_freeform(w, "INVITE", &[n, c]),
            Command::KICK(c, n, Some(r)) => write_cmd_freeform(w, "KICK", &[c, n, r]),
            Command::KICK(c, n, None) => write_cmd(w, "KICK", &[c, n]),

            // Messaging
            Command::PRIVMSG(t, m) => write_cmd_freeform(w, "PRIVMSG", &[t, m]),
            Command::NOTICE(t, m) => write_cmd_freeform(w, "NOTICE", &[t, m]),
            Command::ACCEPT(n) => write_cmd(w, "ACCEPT", &[n]),

            // Server Queries
            Command::MOTD(Some(t)) => write_cmd(w, "MOTD", &[t]),
            Command::MOTD(None) => w.write_str("MOTD"),
            Command::LUSERS(Some(m), Some(t)) => write_cmd(w, "LUSERS", &[m, t]),
            Command::LUSERS(Some(m), None) => write_cmd(w, "LUSERS", &[m]),
            Command::LUSERS(None, _) => w.write_str("LUSERS"),
            Command::VERSION(Some(t)) => write_cmd(w, "VERSION", &[t]),
            Command::VERSION(None) => w.write_str("VERSION"),
            Command::STATS(Some(q), Some(t)) => write_cmd(w, "STATS", &[q, t]),
            Command::STATS(Some(q), None) => write_cmd(w, "STATS", &[q]),
            Command::STATS(None, _) => w.write_str("STATS"),
            Command::LINKS(Some(r), Some(s)) => write_cmd(w, "LINKS", &[r, s]),
            Command::LINKS(None, Some(s)) => write_cmd(w, "LINKS", &[s]),
            Command::LINKS(_, None) => w.write_str("LINKS"),
            Command::TIME(Some(t)) => write_cmd(w, "TIME", &[t]),
            Command::TIME(None) => w.write_str("TIME"),
            Command::CONNECT(t, p, Some(r)) => write_cmd(w, "CONNECT", &[t, p, r]),
            Command::CONNECT(t, p, None) => write_cmd(w, "CONNECT", &[t, p]),
            Command::TRACE(Some(t)) => write_cmd(w, "TRACE", &[t]),
            Command::TRACE(None) => w.write_str("TRACE"),
            Command::ADMIN(Some(t)) => write_cmd(w, "ADMIN", &[t]),
            Command::ADMIN(None) => w.write_str("ADMIN"),
            Command::INFO(Some(t)) => write_cmd(w, "INFO", &[t]),
            Command::INFO(None) => w.write_str("INFO"),
            Command::SID(name, hop, sid, desc) => {
                write_cmd_freeform(w, "SID", &[name, hop, sid, desc])
            }
            Command::UID(nick, hop, ts, user, host, uid, modes, real) => {
                write_cmd_freeform(w, "UID", &[nick, hop, ts, user, host, uid, modes, real])
            }
            Command::SJOIN(ts, channel, modes, args, users) => {
                let mut written = w.write_str("SJOIN ")?;
                written += w.write_str(&ts.to_string())?;
                written += w.write_char(' ')?;
                written += w.write_str(channel)?;
                written += w.write_char(' ')?;
                written += w.write_str(modes)?;
                for arg in args {
                    written += w.write_char(' ')?;
                    written += w.write_str(arg)?;
                }
                written += w.write_str(" :")?;
                for (i, (prefixes, uid)) in users.iter().enumerate() {
                    if i > 0 {
                        written += w.write_char(' ')?;
                    }
                    written += w.write_str(prefixes)?;
                    written += w.write_str(uid)?;
                }
                Ok(written)
            }
            Command::TMODE(ts, channel, modes, args) => {
                let mut written = w.write_str("TMODE ")?;
                written += w.write_str(&ts.to_string())?;
                written += w.write_char(' ')?;
                written += w.write_str(channel)?;
                written += w.write_char(' ')?;
                written += w.write_str(modes)?;
                for arg in args {
                    written += w.write_char(' ')?;
                    written += w.write_str(arg)?;
                }
                Ok(written)
            }
            Command::ENCAP(target, subcommand, params) => {
                let mut written = w.write_str("ENCAP ")?;
                written += w.write_str(target)?;
                written += w.write_char(' ')?;
                written += w.write_str(subcommand)?;
                for (i, param) in params.iter().enumerate() {
                    written += w.write_char(' ')?;
                    let is_last = i == params.len() - 1;
                    if is_last && needs_colon_prefix(param) {
                        written += w.write_char(':')?;
                    }
                    written += w.write_str(param)?;
                }
                Ok(written)
            }
            Command::MAP => w.write_str("MAP"),
            Command::RULES => w.write_str("RULES"),
            Command::EOB => w.write_str("EOB"),
            Command::TB(channel, ts, Some(nick), topic) => {
                write_cmd_freeform(w, "TB", &[channel, &ts.to_string(), nick, topic])
            }
            Command::TB(channel, ts, None, topic) => {
                write_cmd_freeform(w, "TB", &[channel, &ts.to_string(), topic])
            }
            Command::USERIP(u) => {
                let mut written = w.write_str("USERIP")?;
                written += write_service_args(w, u)?;
                Ok(written)
            }
            Command::HELP(Some(t)) => write_cmd(w, "HELP", &[t]),
            Command::HELP(None) => w.write_str("HELP"),
            Command::METADATA {
                subcommand,
                target,
                params,
            } => {
                let mut written = w.write_str("METADATA ")?;
                written += w.write_str(subcommand.as_str())?;
                written += w.write_char(' ')?;
                written += w.write_str(target)?;
                for param in params {
                    written += w.write_char(' ')?;
                    written += w.write_str(param)?;
                }
                Ok(written)
            }
            Command::SERVLIST(Some(m), Some(t)) => write_cmd(w, "SERVLIST", &[m, t]),
            Command::SERVLIST(Some(m), None) => write_cmd(w, "SERVLIST", &[m]),
            Command::SERVLIST(None, _) => w.write_str("SERVLIST"),
            Command::SQUERY(s, t) => write_cmd_freeform(w, "SQUERY", &[s, t]),

            // User Queries
            Command::WHO(Some(s), Some(flags)) => write_cmd(w, "WHO", &[s, flags]),
            Command::WHO(Some(s), None) => write_cmd(w, "WHO", &[s]),
            Command::WHO(None, _) => w.write_str("WHO"),
            Command::WHOIS(Some(t), m) => write_cmd(w, "WHOIS", &[t, m]),
            Command::WHOIS(None, m) => write_cmd(w, "WHOIS", &[m]),
            Command::WHOWAS(n, Some(c), Some(t)) => write_cmd(w, "WHOWAS", &[n, c, t]),
            Command::WHOWAS(n, Some(c), None) => write_cmd(w, "WHOWAS", &[n, c]),
            Command::WHOWAS(n, None, _) => write_cmd(w, "WHOWAS", &[n]),

            // Miscellaneous
            Command::KILL(n, c) => write_cmd_freeform(w, "KILL", &[n, c]),
            Command::PING(s, Some(t)) => write_cmd(w, "PING", &[s, t]),
            Command::PING(s, None) => write_cmd(w, "PING", &[s]),
            Command::PONG(s, Some(t)) => write_cmd(w, "PONG", &[s, t]),
            Command::PONG(s, None) => write_cmd(w, "PONG", &[s]),
            Command::ERROR(m) => write_cmd_freeform(w, "ERROR", &[m]),
            Command::AWAY(Some(m)) => write_cmd_freeform(w, "AWAY", &[m]),
            Command::AWAY(None) => w.write_str("AWAY"),
            Command::REHASH => w.write_str("REHASH"),
            Command::DIE => w.write_str("DIE"),
            Command::RESTART => w.write_str("RESTART"),
            Command::SUMMON(u, Some(t), Some(c)) => write_cmd(w, "SUMMON", &[u, t, c]),
            Command::SUMMON(u, Some(t), None) => write_cmd(w, "SUMMON", &[u, t]),
            Command::SUMMON(u, None, _) => write_cmd(w, "SUMMON", &[u]),
            Command::USERS(Some(t)) => write_cmd(w, "USERS", &[t]),
            Command::USERS(None) => w.write_str("USERS"),
            Command::WALLOPS(t) => write_cmd_freeform(w, "WALLOPS", &[t]),
            Command::GLOBOPS(t) => write_cmd_freeform(w, "GLOBOPS", &[t]),
            Command::USERHOST(u) => {
                let mut written = w.write_str("USERHOST")?;
                written += write_service_args(w, u)?;
                Ok(written)
            }
            Command::ISON(u) => {
                let mut written = w.write_str("ISON")?;
                written += write_service_args(w, u)?;
                Ok(written)
            }

            // Operator Ban Commands
            Command::KLINE(Some(t), m, r) => write_cmd_freeform(w, "KLINE", &[t, m, r]),
            Command::KLINE(None, m, r) => write_cmd_freeform(w, "KLINE", &[m, r]),
            Command::DLINE(Some(t), h, r) => write_cmd_freeform(w, "DLINE", &[t, h, r]),
            Command::DLINE(None, h, r) => write_cmd_freeform(w, "DLINE", &[h, r]),
            Command::UNKLINE(m) => write_cmd(w, "UNKLINE", &[m]),
            Command::UNDLINE(h) => write_cmd(w, "UNDLINE", &[h]),
            Command::GLINE(m, Some(r)) => write_cmd_freeform(w, "GLINE", &[m, r]),
            Command::GLINE(m, None) => write_cmd(w, "GLINE", &[m]),
            Command::UNGLINE(m) => write_cmd(w, "UNGLINE", &[m]),
            Command::ZLINE(ip, Some(r)) => write_cmd_freeform(w, "ZLINE", &[ip, r]),
            Command::ZLINE(ip, None) => write_cmd(w, "ZLINE", &[ip]),
            Command::UNZLINE(ip) => write_cmd(w, "UNZLINE", &[ip]),
            Command::RLINE(p, Some(r)) => write_cmd_freeform(w, "RLINE", &[p, r]),
            Command::RLINE(p, None) => write_cmd(w, "RLINE", &[p]),
            Command::UNRLINE(p) => write_cmd(w, "UNRLINE", &[p]),
            Command::SHUN(m, Some(r)) => write_cmd_freeform(w, "SHUN", &[m, r]),
            Command::SHUN(m, None) => write_cmd(w, "SHUN", &[m]),
            Command::UNSHUN(m) => write_cmd(w, "UNSHUN", &[m]),
            Command::KNOCK(c, Some(m)) => write_cmd_freeform(w, "KNOCK", &[c, m]),
            Command::KNOCK(c, None) => write_cmd(w, "KNOCK", &[c]),

            // Server-to-Server
            Command::SERVER(n, h, t, i) => {
                write_cmd_freeform(w, "SERVER", &[n, &h.to_string(), t, i])
            }
            Command::CAPAB(caps) => {
                let args: Vec<&str> = caps.iter().map(|s| s.as_str()).collect();
                write_cmd(w, "CAPAB", &args)
            }
            Command::SVINFO(v, m, z, t) => write_cmd_freeform(
                w,
                "SVINFO",
                &[
                    &v.to_string(),
                    &m.to_string(),
                    &z.to_string(),
                    &t.to_string(),
                ],
            ),

            // Services Commands
            Command::SAJOIN(n, c) => write_cmd(w, "SAJOIN", &[n, c]),
            Command::SAMODE(t, m, Some(p)) => write_cmd(w, "SAMODE", &[t, m, p]),
            Command::SAMODE(t, m, None) => write_cmd(w, "SAMODE", &[t, m]),
            Command::SANICK(o, n) => write_cmd(w, "SANICK", &[o, n]),
            Command::SAPART(c, r) => write_cmd(w, "SAPART", &[c, r]),
            Command::SAQUIT(c, r) => write_cmd(w, "SAQUIT", &[c, r]),
            Command::NICKSERV(p) => {
                let mut written = w.write_str("NICKSERV")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::CHANSERV(p) => {
                let mut written = w.write_str("CHANSERV")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::OPERSERV(p) => {
                let mut written = w.write_str("OPERSERV")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::BOTSERV(p) => {
                let mut written = w.write_str("BOTSERV")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::HOSTSERV(p) => {
                let mut written = w.write_str("HOSTSERV")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::MEMOSERV(p) => {
                let mut written = w.write_str("MEMOSERV")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::NS(p) => {
                let mut written = w.write_str("NS")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::CS(p) => {
                let mut written = w.write_str("CS")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::OS(p) => {
                let mut written = w.write_str("OS")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::BS(p) => {
                let mut written = w.write_str("BS")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::HS(p) => {
                let mut written = w.write_str("HS")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }
            Command::MS(p) => {
                let mut written = w.write_str("MS")?;
                written += write_service_args(w, p)?;
                Ok(written)
            }

            // IRCv3 Extensions
            Command::CAP(target, subcmd, code, params) => {
                let mut written = w.write_str("CAP")?;
                if let Some(t) = target {
                    written += w.write_char(' ')?;
                    written += w.write_str(t)?;
                }
                written += w.write_char(' ')?;
                written += w.write_str(subcmd.to_str())?;
                if let Some(c) = code {
                    written += w.write_char(' ')?;
                    written += w.write_str(c)?;
                }
                if let Some(p) = params {
                    written += w.write_char(' ')?;
                    written += w.write_str(p)?;
                }
                Ok(written)
            }
            Command::AUTHENTICATE(d) => write_cmd(w, "AUTHENTICATE", &[d]),
            Command::ACCOUNT(a) => write_cmd(w, "ACCOUNT", &[a]),
            Command::MONITOR(c, Some(t)) => write_cmd(w, "MONITOR", &[c, t]),
            Command::MONITOR(c, None) => write_cmd(w, "MONITOR", &[c]),
            Command::BATCH(t, Some(c), Some(a)) => {
                let mut written = w.write_str("BATCH ")?;
                written += w.write_str(t)?;
                written += w.write_char(' ')?;
                written += w.write_str(c.to_str())?;
                written += write_service_args(w, a)?;
                Ok(written)
            }
            Command::BATCH(t, Some(c), None) => write_cmd(w, "BATCH", &[t, c.to_str()]),
            Command::BATCH(t, None, Some(a)) => {
                let mut written = w.write_str("BATCH ")?;
                written += w.write_str(t)?;
                written += write_service_args(w, a)?;
                Ok(written)
            }
            Command::BATCH(t, None, None) => write_cmd(w, "BATCH", &[t]),
            Command::CHGHOST(u, h) => write_cmd(w, "CHGHOST", &[u, h]),
            Command::CHGIDENT(u, i) => write_cmd(w, "CHGIDENT", &[u, i]),
            Command::SETNAME(r) => write_cmd_freeform(w, "SETNAME", &[r]),
            Command::TAGMSG(t) => write_cmd(w, "TAGMSG", &[t]),
            Command::ACK => w.write_str("ACK"),
            Command::WEBIRC(pass, gateway, host, ip, Some(opts)) => {
                write_cmd(w, "WEBIRC", &[pass, gateway, host, ip, opts])
            }
            Command::WEBIRC(pass, gateway, host, ip, None) => {
                write_cmd(w, "WEBIRC", &[pass, gateway, host, ip])
            }
            Command::CHATHISTORY {
                subcommand,
                target,
                msg_ref1,
                msg_ref2,
                limit,
            } => {
                let mut written = w.write_str("CHATHISTORY ")?;
                let subcmd_str = subcommand.to_string();
                written += w.write_str(&subcmd_str)?;

                match subcommand {
                    ChatHistorySubCommand::TARGETS => {
                        let ref1_str = msg_ref1.to_string();
                        written += w.write_char(' ')?;
                        written += w.write_str(&ref1_str)?;
                        if let Some(ref2) = msg_ref2 {
                            let ref2_str = ref2.to_string();
                            written += w.write_char(' ')?;
                            written += w.write_str(&ref2_str)?;
                        }
                        written += w.write_char(' ')?;
                        let limit_str = limit.to_string();
                        written += w.write_str(&limit_str)?;
                    }
                    ChatHistorySubCommand::BETWEEN => {
                        let ref1_str = msg_ref1.to_string();
                        written += w.write_char(' ')?;
                        written += w.write_str(target)?;
                        written += w.write_char(' ')?;
                        written += w.write_str(&ref1_str)?;
                        if let Some(ref2) = msg_ref2 {
                            let ref2_str = ref2.to_string();
                            written += w.write_char(' ')?;
                            written += w.write_str(&ref2_str)?;
                        }
                        written += w.write_char(' ')?;
                        let limit_str = limit.to_string();
                        written += w.write_str(&limit_str)?;
                    }
                    _ => {
                        let ref1_str = msg_ref1.to_string();
                        let limit_str = limit.to_string();
                        written += w.write_char(' ')?;
                        written += w.write_str(target)?;
                        written += w.write_char(' ')?;
                        written += w.write_str(&ref1_str)?;
                        written += w.write_char(' ')?;
                        written += w.write_str(&limit_str)?;
                    }
                }
                Ok(written)
            }
            Command::ChatHistoryTargets { target, timestamp } => {
                write_cmd(w, "CHATHISTORY", &["TARGETS", target, timestamp])
            }

            Command::NPC {
                channel,
                nick,
                text,
            } => write_cmd_freeform(w, "NPC", &[channel, nick, text]),
            Command::RELAYMSG {
                relay_from,
                target,
                text,
            } => write_cmd_freeform(w, "RELAYMSG", &[target, relay_from, text]),

            // Standard Replies
            Command::FAIL(command, code, context) => {
                write_standard_reply(w, "FAIL", command, code, context)
            }
            Command::WARN(command, code, context) => {
                write_standard_reply(w, "WARN", command, code, context)
            }
            Command::NOTE(command, code, context) => {
                write_standard_reply(w, "NOTE", command, code, context)
            }

            Command::REGISTER { account, message } => {
                let mut written = w.write_str("REGISTER SUCCESS ")?;
                written += w.write_str(account)?;
                if let Some(msg) = message {
                    written += w.write_str(" :")?;
                    written += w.write_str(msg)?;
                }
                Ok(written)
            }

            // Numeric Response
            Command::Response(resp, args) => {
                let code = *resp as u16;
                let mut written = w.write_fmt(format_args!("{:03}", code))?;

                let len = args.len();
                for (i, arg) in args.iter().enumerate() {
                    written += w.write_char(' ')?;
                    if i == len - 1 && needs_colon_prefix(arg) {
                        written += w.write_char(':')?;
                    }
                    written += w.write_str(arg)?;
                }
                Ok(written)
            }

            // Raw
            Command::Raw(cmd, args) => {
                let mut written = w.write_str(cmd)?;
                written += write_args_with_trailing(w, args.iter().map(String::as_str))?;
                Ok(written)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to encode a command to bytes and return as UTF-8 string.
    fn encode_cmd(cmd: Command) -> String {
        let mut buf = Vec::new();
        cmd.encode(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    // Basic commands
    #[test]
    fn test_encode_pass() {
        assert_eq!(encode_cmd(Command::PASS("secret".into())), "PASS secret");
    }

    #[test]
    fn test_encode_nick() {
        assert_eq!(
            encode_cmd(Command::NICK("testnick".into())),
            "NICK testnick"
        );
    }

    #[test]
    fn test_encode_user() {
        assert_eq!(
            encode_cmd(Command::USER("user".into(), "0".into(), "Real Name".into())),
            "USER user 0 * :Real Name"
        );
    }

    #[test]
    fn test_encode_oper() {
        assert_eq!(
            encode_cmd(Command::OPER("admin".into(), "secret".into())),
            "OPER admin secret"
        );
    }

    #[test]
    fn test_encode_quit_with_message() {
        assert_eq!(
            encode_cmd(Command::QUIT(Some("Goodbye".into()))),
            "QUIT Goodbye"
        );
    }

    #[test]
    fn test_encode_quit_with_message_space() {
        // Messages with spaces need colon prefix
        assert_eq!(
            encode_cmd(Command::QUIT(Some("Goodbye world".into()))),
            "QUIT :Goodbye world"
        );
    }

    #[test]
    fn test_encode_quit_no_message() {
        assert_eq!(encode_cmd(Command::QUIT(None)), "QUIT");
    }

    // Channel operations
    #[test]
    fn test_encode_join_simple() {
        assert_eq!(
            encode_cmd(Command::JOIN("#channel".into(), None, None)),
            "JOIN #channel"
        );
    }

    #[test]
    fn test_encode_join_with_key() {
        assert_eq!(
            encode_cmd(Command::JOIN("#channel".into(), Some("key".into()), None)),
            "JOIN #channel key"
        );
    }

    #[test]
    fn test_encode_part_with_message() {
        assert_eq!(
            encode_cmd(Command::PART("#channel".into(), Some("Leaving".into()))),
            "PART #channel :Leaving"
        );
    }

    #[test]
    fn test_encode_part_no_message() {
        assert_eq!(
            encode_cmd(Command::PART("#channel".into(), None)),
            "PART #channel"
        );
    }

    #[test]
    fn test_encode_topic_set() {
        assert_eq!(
            encode_cmd(Command::TOPIC("#channel".into(), Some("New Topic".into()))),
            "TOPIC #channel :New Topic"
        );
    }

    #[test]
    fn test_encode_topic_query() {
        assert_eq!(
            encode_cmd(Command::TOPIC("#channel".into(), None)),
            "TOPIC #channel"
        );
    }

    #[test]
    fn test_encode_names() {
        assert_eq!(
            encode_cmd(Command::NAMES(Some("#channel".into()), None)),
            "NAMES #channel"
        );
    }

    #[test]
    fn test_encode_names_no_args() {
        assert_eq!(encode_cmd(Command::NAMES(None, None)), "NAMES");
    }

    #[test]
    fn test_encode_list() {
        assert_eq!(
            encode_cmd(Command::LIST(Some("#channel".into()), None)),
            "LIST #channel"
        );
    }

    #[test]
    fn test_encode_invite() {
        assert_eq!(
            encode_cmd(Command::INVITE("nick".into(), "#channel".into())),
            "INVITE nick :#channel"
        );
    }

    #[test]
    fn test_encode_kick_with_reason() {
        assert_eq!(
            encode_cmd(Command::KICK(
                "#channel".into(),
                "nick".into(),
                Some("Reason".into())
            )),
            "KICK #channel nick :Reason"
        );
    }

    #[test]
    fn test_encode_kick_no_reason() {
        assert_eq!(
            encode_cmd(Command::KICK("#channel".into(), "nick".into(), None)),
            "KICK #channel nick"
        );
    }

    // Messaging
    #[test]
    fn test_encode_privmsg() {
        assert_eq!(
            encode_cmd(Command::PRIVMSG("#channel".into(), "Hello world".into())),
            "PRIVMSG #channel :Hello world"
        );
    }

    #[test]
    fn test_encode_notice() {
        assert_eq!(
            encode_cmd(Command::NOTICE("nick".into(), "You have mail".into())),
            "NOTICE nick :You have mail"
        );
    }

    // Server queries
    #[test]
    fn test_encode_motd() {
        assert_eq!(encode_cmd(Command::MOTD(None)), "MOTD");
    }

    #[test]
    fn test_encode_lusers() {
        assert_eq!(encode_cmd(Command::LUSERS(None, None)), "LUSERS");
    }

    #[test]
    fn test_encode_version() {
        assert_eq!(encode_cmd(Command::VERSION(None)), "VERSION");
    }

    #[test]
    fn test_encode_stats() {
        assert_eq!(
            encode_cmd(Command::STATS(Some("o".into()), None)),
            "STATS o"
        );
    }

    #[test]
    fn test_encode_links() {
        assert_eq!(encode_cmd(Command::LINKS(None, None)), "LINKS");
    }

    #[test]
    fn test_encode_time() {
        assert_eq!(encode_cmd(Command::TIME(None)), "TIME");
    }

    #[test]
    fn test_encode_admin() {
        assert_eq!(encode_cmd(Command::ADMIN(None)), "ADMIN");
    }

    #[test]
    fn test_encode_info() {
        assert_eq!(encode_cmd(Command::INFO(None)), "INFO");
    }

    // User queries
    #[test]
    fn test_encode_who() {
        assert_eq!(
            encode_cmd(Command::WHO(Some("#channel".into()), None)),
            "WHO #channel"
        );
    }

    #[test]
    fn test_encode_who_with_flags() {
        assert_eq!(
            encode_cmd(Command::WHO(Some("#channel".into()), Some("%nuhaf".into()))),
            "WHO #channel %nuhaf"
        );
    }

    #[test]
    fn test_encode_whois() {
        assert_eq!(
            encode_cmd(Command::WHOIS(None, "nick".into())),
            "WHOIS nick"
        );
    }

    #[test]
    fn test_encode_whowas() {
        assert_eq!(
            encode_cmd(Command::WHOWAS("nick".into(), None, None)),
            "WHOWAS nick"
        );
    }

    // Miscellaneous
    #[test]
    fn test_encode_ping() {
        assert_eq!(
            encode_cmd(Command::PING("server".into(), None)),
            "PING server"
        );
    }

    #[test]
    fn test_encode_pong() {
        assert_eq!(
            encode_cmd(Command::PONG("server".into(), None)),
            "PONG server"
        );
    }

    #[test]
    fn test_encode_away_with_message() {
        assert_eq!(
            encode_cmd(Command::AWAY(Some("Gone fishing".into()))),
            "AWAY :Gone fishing"
        );
    }

    #[test]
    fn test_encode_away_clear() {
        assert_eq!(encode_cmd(Command::AWAY(None)), "AWAY");
    }

    #[test]
    fn test_encode_rehash() {
        assert_eq!(encode_cmd(Command::REHASH), "REHASH");
    }

    #[test]
    fn test_encode_die() {
        assert_eq!(encode_cmd(Command::DIE), "DIE");
    }

    #[test]
    fn test_encode_restart() {
        assert_eq!(encode_cmd(Command::RESTART), "RESTART");
    }

    #[test]
    fn test_encode_wallops() {
        assert_eq!(
            encode_cmd(Command::WALLOPS("Broadcast message".into())),
            "WALLOPS :Broadcast message"
        );
    }

    #[test]
    fn test_encode_userhost() {
        assert_eq!(
            encode_cmd(Command::USERHOST(vec!["nick1".into(), "nick2".into()])),
            "USERHOST nick1 nick2"
        );
    }

    #[test]
    fn test_encode_ison() {
        assert_eq!(
            encode_cmd(Command::ISON(vec!["nick1".into(), "nick2".into()])),
            "ISON nick1 nick2"
        );
    }

    // IRCv3 extensions
    #[test]
    fn test_encode_cap_ls() {
        assert_eq!(
            encode_cmd(Command::CAP(
                None,
                crate::command::CapSubCommand::LS,
                Some("302".into()),
                None
            )),
            "CAP LS 302"
        );
    }

    #[test]
    fn test_encode_cap_req() {
        assert_eq!(
            encode_cmd(Command::CAP(
                Some("*".into()),
                crate::command::CapSubCommand::REQ,
                None,
                Some("multi-prefix".into())
            )),
            "CAP * REQ multi-prefix"
        );
    }

    #[test]
    fn test_encode_cap_end() {
        assert_eq!(
            encode_cmd(Command::CAP(
                None,
                crate::command::CapSubCommand::END,
                None,
                None
            )),
            "CAP END"
        );
    }

    #[test]
    fn test_encode_authenticate() {
        assert_eq!(
            encode_cmd(Command::AUTHENTICATE("PLAIN".into())),
            "AUTHENTICATE PLAIN"
        );
    }

    #[test]
    fn test_encode_account() {
        assert_eq!(
            encode_cmd(Command::ACCOUNT("accountname".into())),
            "ACCOUNT accountname"
        );
    }

    #[test]
    fn test_encode_chghost() {
        assert_eq!(
            encode_cmd(Command::CHGHOST("newuser".into(), "newhost".into())),
            "CHGHOST newuser newhost"
        );
    }

    #[test]
    fn test_encode_setname() {
        assert_eq!(
            encode_cmd(Command::SETNAME("New Real Name".into())),
            "SETNAME :New Real Name"
        );
    }

    #[test]
    fn test_encode_tagmsg() {
        assert_eq!(
            encode_cmd(Command::TAGMSG("#channel".into())),
            "TAGMSG #channel"
        );
    }

    #[test]
    fn test_encode_monitor() {
        assert_eq!(
            encode_cmd(Command::MONITOR("+".into(), Some("nick1,nick2".into()))),
            "MONITOR + nick1,nick2"
        );
    }

    // Standard replies
    #[test]
    fn test_encode_fail() {
        assert_eq!(
            encode_cmd(Command::FAIL(
                "COMMAND".into(),
                "CODE".into(),
                vec!["context".into(), "message".into()]
            )),
            "FAIL COMMAND CODE context :message"
        );
    }

    #[test]
    fn test_encode_warn() {
        assert_eq!(
            encode_cmd(Command::WARN(
                "CMD".into(),
                "WARN_CODE".into(),
                vec!["warning message".into()]
            )),
            "WARN CMD WARN_CODE :warning message"
        );
    }

    #[test]
    fn test_encode_note() {
        assert_eq!(
            encode_cmd(Command::NOTE(
                "CMD".into(),
                "INFO".into(),
                vec!["informational".into()]
            )),
            "NOTE CMD INFO :informational"
        );
    }

    #[test]
    fn test_encode_register() {
        assert_eq!(
            encode_cmd(Command::REGISTER {
                account: "user".into(),
                message: Some("Account created".into())
            }),
            "REGISTER SUCCESS user :Account created"
        );
    }

    #[test]
    fn test_encode_register_no_message() {
        assert_eq!(
            encode_cmd(Command::REGISTER {
                account: "user".into(),
                message: None
            }),
            "REGISTER SUCCESS user"
        );
    }

    // Numeric response
    #[test]
    fn test_encode_response_numeric() {
        assert_eq!(
            encode_cmd(Command::Response(
                crate::response::Response::RPL_WELCOME,
                vec!["nick".into(), "Welcome to the network".into()]
            )),
            "001 nick :Welcome to the network"
        );
    }

    // Raw command
    #[test]
    fn test_encode_raw() {
        assert_eq!(
            encode_cmd(Command::Raw(
                "CUSTOM".into(),
                vec!["arg1".into(), "arg2".into()]
            )),
            "CUSTOM arg1 arg2"
        );
    }

    #[test]
    fn test_encode_raw_with_trailing() {
        assert_eq!(
            encode_cmd(Command::Raw(
                "CUSTOM".into(),
                vec!["arg1".into(), "with space".into()]
            )),
            "CUSTOM arg1 :with space"
        );
    }
}
