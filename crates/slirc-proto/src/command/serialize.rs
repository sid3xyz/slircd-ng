use std::fmt::{self, Write};

use super::types::Command;
use super::util::{
    needs_colon_prefix, write_args_with_trailing, write_cmd, write_cmd_freeform,
    write_collapsed_mode_flags, write_service_args, write_standard_reply,
};

/// Write a service command with variable arguments (e.g., NICKSERV, CHANSERV, NS, CS).
fn write_service_command(f: &mut fmt::Formatter<'_>, cmd: &str, args: &[String]) -> fmt::Result {
    f.write_str(cmd)?;
    write_service_args(f, args).map(|_| ())
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::PASS(p) => write_cmd(f, "PASS", &[p]).map(|_| ()),
            Command::PassTs6 { password, sid } => {
                write_cmd_freeform(f, "PASS", &[password, "TS", "6", sid]).map(|_| ())
            }
            Command::NICK(n) => write_cmd(f, "NICK", &[n]).map(|_| ()),
            Command::USER(u, m, r) => write_cmd_freeform(f, "USER", &[u, m, "*", r]).map(|_| ()),
            Command::OPER(u, p) => write_cmd(f, "OPER", &[u, p]).map(|_| ()),
            Command::UserMODE(u, modes) => {
                f.write_str("MODE ")?;
                f.write_str(u)?;
                if !modes.is_empty() {
                    f.write_char(' ')?;
                    write_collapsed_mode_flags(f, modes)?;
                }
                Ok(())
            }
            Command::SERVICE(nick, r0, dist, typ, r1, info) => {
                write_cmd_freeform(f, "SERVICE", &[nick, r0, dist, typ, r1, info]).map(|_| ())
            }
            Command::QUIT(Some(m)) => write_cmd(f, "QUIT", &[m]).map(|_| ()),
            Command::QUIT(None) => write_cmd(f, "QUIT", &[]).map(|_| ()),
            Command::SQUIT(s, c) => write_cmd_freeform(f, "SQUIT", &[s, c]).map(|_| ()),
            Command::JOIN(c, Some(k), Some(n)) => write_cmd(f, "JOIN", &[c, k, n]).map(|_| ()),
            Command::JOIN(c, Some(k), None) => write_cmd(f, "JOIN", &[c, k]).map(|_| ()),
            Command::JOIN(c, None, Some(n)) => write_cmd(f, "JOIN", &[c, n]).map(|_| ()),
            Command::JOIN(c, None, None) => write_cmd(f, "JOIN", &[c]).map(|_| ()),
            Command::PART(c, Some(m)) => write_cmd_freeform(f, "PART", &[c, m]).map(|_| ()),
            Command::PART(c, None) => write_cmd(f, "PART", &[c]).map(|_| ()),
            Command::ChannelMODE(c, modes) => {
                f.write_str("MODE ")?;
                f.write_str(c)?;
                if !modes.is_empty() {
                    f.write_char(' ')?;
                    write_collapsed_mode_flags(f, modes)?;
                    let mode_args: Vec<_> = modes.iter().filter_map(|m| m.arg()).collect();
                    for (i, arg) in mode_args.iter().enumerate() {
                        super::util::validate_param(f, arg)?;
                        f.write_char(' ')?;
                        // Last argument needs colon prefix if it contains space, is empty, or starts with ':'
                        let is_last = i == mode_args.len() - 1;
                        if is_last && needs_colon_prefix(arg) {
                            f.write_char(':')?;
                        }
                        f.write_str(arg)?;
                    }
                }
                Ok(())
            }
            Command::TOPIC(c, Some(t)) => write_cmd_freeform(f, "TOPIC", &[c, t]).map(|_| ()),
            Command::TOPIC(c, None) => write_cmd(f, "TOPIC", &[c]).map(|_| ()),
            Command::NAMES(Some(c), Some(t)) => write_cmd(f, "NAMES", &[c, t]).map(|_| ()),
            Command::NAMES(Some(c), None) => write_cmd(f, "NAMES", &[c]).map(|_| ()),
            Command::NAMES(None, _) => write_cmd(f, "NAMES", &[]).map(|_| ()),
            Command::LIST(Some(c), Some(t)) => write_cmd(f, "LIST", &[c, t]).map(|_| ()),
            Command::LIST(Some(c), None) => write_cmd(f, "LIST", &[c]).map(|_| ()),
            Command::LIST(None, _) => write_cmd(f, "LIST", &[]).map(|_| ()),
            Command::INVITE(n, c) => write_cmd_freeform(f, "INVITE", &[n, c]).map(|_| ()),
            Command::KICK(c, n, Some(r)) => write_cmd_freeform(f, "KICK", &[c, n, r]).map(|_| ()),
            Command::KICK(c, n, None) => write_cmd(f, "KICK", &[c, n]).map(|_| ()),
            Command::PRIVMSG(t, m) => write_cmd_freeform(f, "PRIVMSG", &[t, m]).map(|_| ()),
            Command::NOTICE(t, m) => write_cmd_freeform(f, "NOTICE", &[t, m]).map(|_| ()),
            Command::ACCEPT(n) => write_cmd(f, "ACCEPT", &[n]).map(|_| ()),
            Command::MOTD(Some(t)) => write_cmd(f, "MOTD", &[t]).map(|_| ()),
            Command::MOTD(None) => write_cmd(f, "MOTD", &[]).map(|_| ()),
            Command::LUSERS(Some(m), Some(t)) => write_cmd(f, "LUSERS", &[m, t]).map(|_| ()),
            Command::LUSERS(Some(m), None) => write_cmd(f, "LUSERS", &[m]).map(|_| ()),
            Command::LUSERS(None, _) => write_cmd(f, "LUSERS", &[]).map(|_| ()),
            Command::VERSION(Some(t)) => write_cmd(f, "VERSION", &[t]).map(|_| ()),
            Command::VERSION(None) => write_cmd(f, "VERSION", &[]).map(|_| ()),
            Command::STATS(Some(q), Some(t)) => write_cmd(f, "STATS", &[q, t]).map(|_| ()),
            Command::STATS(Some(q), None) => write_cmd(f, "STATS", &[q]).map(|_| ()),
            Command::STATS(None, _) => write_cmd(f, "STATS", &[]).map(|_| ()),
            Command::LINKS(Some(r), Some(s)) => write_cmd(f, "LINKS", &[r, s]).map(|_| ()),
            Command::LINKS(None, Some(s)) => write_cmd(f, "LINKS", &[s]).map(|_| ()),
            Command::LINKS(_, None) => write_cmd(f, "LINKS", &[]).map(|_| ()),
            Command::TIME(Some(t)) => write_cmd(f, "TIME", &[t]).map(|_| ()),
            Command::TIME(None) => write_cmd(f, "TIME", &[]).map(|_| ()),
            Command::CONNECT(t, p, Some(r)) => write_cmd(f, "CONNECT", &[t, p, r]).map(|_| ()),
            Command::CONNECT(t, p, None) => write_cmd(f, "CONNECT", &[t, p]).map(|_| ()),
            Command::TRACE(Some(t)) => write_cmd(f, "TRACE", &[t]).map(|_| ()),
            Command::TRACE(None) => write_cmd(f, "TRACE", &[]).map(|_| ()),
            Command::ADMIN(Some(t)) => write_cmd(f, "ADMIN", &[t]).map(|_| ()),
            Command::ADMIN(None) => write_cmd(f, "ADMIN", &[]).map(|_| ()),
            Command::INFO(Some(t)) => write_cmd(f, "INFO", &[t]).map(|_| ()),
            Command::INFO(None) => write_cmd(f, "INFO", &[]).map(|_| ()),
            Command::SID(name, hop, sid, desc) => {
                write_cmd_freeform(f, "SID", &[name, hop, sid, desc]).map(|_| ())
            }
            Command::UID(nick, hop, ts, user, host, uid, modes, real) => {
                write_cmd_freeform(f, "UID", &[nick, hop, ts, user, host, uid, modes, real])
                    .map(|_| ())
            }
            Command::SJOIN(ts, channel, modes, args, users) => {
                f.write_str("SJOIN ")?;
                write!(f, "{} {} {}", ts, channel, modes)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                f.write_str(" :")?;
                for (i, (prefixes, uid)) in users.iter().enumerate() {
                    if i > 0 {
                        f.write_char(' ')?;
                    }
                    f.write_str(prefixes)?;
                    f.write_str(uid)?;
                }
                Ok(())
            }
            Command::TMODE(ts, channel, modes, args) => {
                f.write_str("TMODE ")?;
                write!(f, "{} {} {}", ts, channel, modes)?;
                for arg in args {
                    write!(f, " {}", arg)?;
                }
                Ok(())
            }
            Command::ENCAP(target, subcommand, params) => {
                f.write_str("ENCAP ")?;
                f.write_str(target)?;
                f.write_char(' ')?;
                f.write_str(subcommand)?;
                for (i, param) in params.iter().enumerate() {
                    f.write_char(' ')?;
                    let is_last = i == params.len() - 1;
                    if is_last && needs_colon_prefix(param) {
                        f.write_char(':')?;
                    }
                    f.write_str(param)?;
                }
                Ok(())
            }
            Command::MAP => write_cmd(f, "MAP", &[]).map(|_| ()),
            Command::RULES => write_cmd(f, "RULES", &[]).map(|_| ()),
            Command::EOB => write_cmd(f, "EOB", &[]).map(|_| ()),
            Command::TB(channel, ts, Some(nick), topic) => {
                write_cmd_freeform(f, "TB", &[channel, &ts.to_string(), nick, topic]).map(|_| ())
            }
            Command::TB(channel, ts, None, topic) => {
                write_cmd_freeform(f, "TB", &[channel, &ts.to_string(), topic]).map(|_| ())
            }
            Command::USERIP(u) => {
                f.write_str("USERIP")?;
                for nick in u {
                    f.write_char(' ')?;
                    f.write_str(nick)?;
                }
                Ok(())
            }
            Command::HELP(Some(t)) => write_cmd(f, "HELP", &[t]).map(|_| ()),
            Command::HELP(None) => write_cmd(f, "HELP", &[]).map(|_| ()),
            Command::METADATA {
                subcommand,
                target,
                params,
            } => {
                f.write_str("METADATA ")?;
                f.write_str(subcommand.as_str())?;
                f.write_char(' ')?;
                f.write_str(target)?;
                for param in params {
                    f.write_char(' ')?;
                    f.write_str(param)?;
                }
                Ok(())
            }
            Command::SERVLIST(Some(m), Some(t)) => write_cmd(f, "SERVLIST", &[m, t]).map(|_| ()),
            Command::SERVLIST(Some(m), None) => write_cmd(f, "SERVLIST", &[m]).map(|_| ()),
            Command::SERVLIST(None, _) => write_cmd(f, "SERVLIST", &[]).map(|_| ()),
            Command::SQUERY(s, t) => write_cmd_freeform(f, "SQUERY", &[s, t]).map(|_| ()),
            Command::WHO(Some(s), Some(flags)) => write_cmd(f, "WHO", &[s, flags]).map(|_| ()),
            Command::WHO(Some(s), None) => write_cmd(f, "WHO", &[s]).map(|_| ()),
            Command::WHO(None, _) => write_cmd(f, "WHO", &[]).map(|_| ()),
            Command::WHOIS(Some(t), m) => write_cmd(f, "WHOIS", &[t, m]).map(|_| ()),
            Command::WHOIS(None, m) => write_cmd(f, "WHOIS", &[m]).map(|_| ()),
            Command::WHOWAS(n, Some(c), Some(t)) => write_cmd(f, "WHOWAS", &[n, c, t]).map(|_| ()),
            Command::WHOWAS(n, Some(c), None) => write_cmd(f, "WHOWAS", &[n, c]).map(|_| ()),
            Command::WHOWAS(n, None, Some(c)) => write_cmd(f, "WHOWAS", &[n, c]).map(|_| ()),
            Command::WHOWAS(n, None, None) => write_cmd(f, "WHOWAS", &[n]).map(|_| ()),
            Command::KILL(n, c) => write_cmd_freeform(f, "KILL", &[n, c]).map(|_| ()),
            Command::PING(s, Some(t)) => write_cmd(f, "PING", &[s, t]).map(|_| ()),
            Command::PING(s, None) => write_cmd(f, "PING", &[s]).map(|_| ()),
            Command::PONG(s, Some(t)) => write_cmd(f, "PONG", &[s, t]).map(|_| ()),
            Command::PONG(s, None) => write_cmd(f, "PONG", &[s]).map(|_| ()),
            Command::ERROR(m) => write_cmd_freeform(f, "ERROR", &[m]).map(|_| ()),
            Command::AWAY(Some(m)) => write_cmd_freeform(f, "AWAY", &[m]).map(|_| ()),
            Command::AWAY(None) => write_cmd(f, "AWAY", &[]).map(|_| ()),
            Command::REHASH => write_cmd(f, "REHASH", &[]).map(|_| ()),
            Command::DIE => write_cmd(f, "DIE", &[]).map(|_| ()),
            Command::RESTART => write_cmd(f, "RESTART", &[]).map(|_| ()),
            Command::SUMMON(u, Some(t), Some(c)) => write_cmd(f, "SUMMON", &[u, t, c]).map(|_| ()),
            Command::SUMMON(u, Some(t), None) => write_cmd(f, "SUMMON", &[u, t]).map(|_| ()),
            Command::SUMMON(u, None, _) => write_cmd(f, "SUMMON", &[u]).map(|_| ()),
            Command::USERS(Some(t)) => write_cmd(f, "USERS", &[t]).map(|_| ()),
            Command::USERS(None) => write_cmd(f, "USERS", &[]).map(|_| ()),
            Command::WALLOPS(t) => write_cmd_freeform(f, "WALLOPS", &[t]).map(|_| ()),
            Command::GLOBOPS(t) => write_cmd_freeform(f, "GLOBOPS", &[t]).map(|_| ()),
            Command::USERHOST(u) => {
                f.write_str("USERHOST")?;
                write_args_with_trailing(f, u.iter().map(String::as_str)).map(|_| ())
            }
            Command::ISON(u) => {
                f.write_str("ISON")?;
                write_args_with_trailing(f, u.iter().map(String::as_str)).map(|_| ())
            }
            Command::SAJOIN(n, c) => write_cmd(f, "SAJOIN", &[n, c]).map(|_| ()),
            Command::SAMODE(t, m, Some(p)) => write_cmd(f, "SAMODE", &[t, m, p]).map(|_| ()),
            Command::SAMODE(t, m, None) => write_cmd(f, "SAMODE", &[t, m]).map(|_| ()),
            Command::SANICK(o, n) => write_cmd(f, "SANICK", &[o, n]).map(|_| ()),
            Command::SAPART(c, r) => write_cmd(f, "SAPART", &[c, r]).map(|_| ()),
            Command::SAQUIT(c, r) => write_cmd(f, "SAQUIT", &[c, r]).map(|_| ()),
            Command::KLINE(Some(t), m, r) => write_cmd_freeform(f, "KLINE", &[t, m, r]).map(|_| ()),
            Command::KLINE(None, m, r) => write_cmd_freeform(f, "KLINE", &[m, r]).map(|_| ()),
            Command::DLINE(Some(t), h, r) => write_cmd_freeform(f, "DLINE", &[t, h, r]).map(|_| ()),
            Command::DLINE(None, h, r) => write_cmd_freeform(f, "DLINE", &[h, r]).map(|_| ()),
            Command::UNKLINE(m) => write_cmd(f, "UNKLINE", &[m]).map(|_| ()),
            Command::UNDLINE(h) => write_cmd(f, "UNDLINE", &[h]).map(|_| ()),
            Command::GLINE(m, Some(r)) => write_cmd_freeform(f, "GLINE", &[m, r]).map(|_| ()),
            Command::GLINE(m, None) => write_cmd(f, "GLINE", &[m]).map(|_| ()),
            Command::UNGLINE(m) => write_cmd(f, "UNGLINE", &[m]).map(|_| ()),
            Command::ZLINE(ip, Some(r)) => write_cmd_freeform(f, "ZLINE", &[ip, r]).map(|_| ()),
            Command::ZLINE(ip, None) => write_cmd(f, "ZLINE", &[ip]).map(|_| ()),
            Command::UNZLINE(ip) => write_cmd(f, "UNZLINE", &[ip]).map(|_| ()),
            Command::RLINE(p, Some(r)) => write_cmd_freeform(f, "RLINE", &[p, r]).map(|_| ()),
            Command::RLINE(p, None) => write_cmd(f, "RLINE", &[p]).map(|_| ()),
            Command::UNRLINE(p) => write_cmd(f, "UNRLINE", &[p]).map(|_| ()),
            Command::SHUN(m, Some(r)) => write_cmd_freeform(f, "SHUN", &[m, r]).map(|_| ()),
            Command::SHUN(m, None) => write_cmd(f, "SHUN", &[m]).map(|_| ()),
            Command::UNSHUN(m) => write_cmd(f, "UNSHUN", &[m]).map(|_| ()),
            Command::KNOCK(c, Some(m)) => write_cmd_freeform(f, "KNOCK", &[c, m]).map(|_| ()),
            Command::KNOCK(c, None) => write_cmd(f, "KNOCK", &[c]).map(|_| ()),
            Command::SERVER(n, h, t, i) => {
                write_cmd_freeform(f, "SERVER", &[n, &h.to_string(), t, i]).map(|_| ())
            }
            Command::CAPAB(caps) => {
                let args: Vec<&str> = caps.iter().map(|s| s.as_str()).collect();
                write_cmd(f, "CAPAB", &args).map(|_| ())
            }
            Command::SVINFO(v, m, z, t) => write_cmd_freeform(
                f,
                "SVINFO",
                &[
                    &v.to_string(),
                    &m.to_string(),
                    &z.to_string(),
                    &t.to_string(),
                ],
            )
            .map(|_| ()),
            Command::NICKSERV(p) => write_service_command(f, "NICKSERV", p),
            Command::CHANSERV(p) => write_service_command(f, "CHANSERV", p),
            Command::OPERSERV(p) => write_service_command(f, "OPERSERV", p),
            Command::BOTSERV(p) => write_service_command(f, "BOTSERV", p),
            Command::HOSTSERV(p) => write_service_command(f, "HOSTSERV", p),
            Command::MEMOSERV(p) => write_service_command(f, "MEMOSERV", p),
            Command::NS(p) => write_service_command(f, "NS", p),
            Command::CS(p) => write_service_command(f, "CS", p),
            Command::OS(p) => write_service_command(f, "OS", p),
            Command::BS(p) => write_service_command(f, "BS", p),
            Command::HS(p) => write_service_command(f, "HS", p),
            Command::MS(p) => write_service_command(f, "MS", p),
            Command::CAP(None, s, None, Some(p)) => {
                write_cmd(f, "CAP", &[s.to_str(), p]).map(|_| ())
            }
            Command::CAP(None, s, None, None) => write_cmd(f, "CAP", &[s.to_str()]).map(|_| ()),
            Command::CAP(Some(k), s, None, Some(p)) => {
                write_cmd(f, "CAP", &[k, s.to_str(), p]).map(|_| ())
            }
            Command::CAP(Some(k), s, None, None) => {
                write_cmd(f, "CAP", &[k, s.to_str()]).map(|_| ())
            }
            Command::CAP(None, s, Some(c), Some(p)) => {
                write_cmd(f, "CAP", &[s.to_str(), c, p]).map(|_| ())
            }
            Command::CAP(None, s, Some(c), None) => {
                write_cmd(f, "CAP", &[s.to_str(), c]).map(|_| ())
            }
            Command::CAP(Some(k), s, Some(c), Some(p)) => {
                write_cmd(f, "CAP", &[k, s.to_str(), c, p]).map(|_| ())
            }
            Command::CAP(Some(k), s, Some(c), None) => {
                write_cmd(f, "CAP", &[k, s.to_str(), c]).map(|_| ())
            }
            Command::AUTHENTICATE(d) => write_cmd(f, "AUTHENTICATE", &[d]).map(|_| ()),
            Command::ACCOUNT(a) => write_cmd(f, "ACCOUNT", &[a]).map(|_| ()),
            Command::MONITOR(c, Some(t)) => write_cmd(f, "MONITOR", &[c, t]).map(|_| ()),
            Command::MONITOR(c, None) => write_cmd(f, "MONITOR", &[c]).map(|_| ()),
            Command::BATCH(t, Some(c), Some(a)) => {
                f.write_str("BATCH ")?;
                f.write_str(t)?;
                f.write_char(' ')?;
                f.write_str(c.to_str())?;
                write_args_with_trailing(f, a.iter().map(String::as_str)).map(|_| ())
            }
            Command::BATCH(t, Some(c), None) => write_cmd(f, "BATCH", &[t, c.to_str()]).map(|_| ()),
            Command::BATCH(t, None, Some(a)) => {
                f.write_str("BATCH ")?;
                f.write_str(t)?;
                write_args_with_trailing(f, a.iter().map(String::as_str)).map(|_| ())
            }
            Command::BATCH(t, None, None) => write_cmd(f, "BATCH", &[t]).map(|_| ()),
            Command::CHGHOST(u, h) => write_cmd(f, "CHGHOST", &[u, h]).map(|_| ()),
            Command::CHGIDENT(u, i) => write_cmd(f, "CHGIDENT", &[u, i]).map(|_| ()),
            Command::SETNAME(r) => write_cmd_freeform(f, "SETNAME", &[r]).map(|_| ()),
            Command::TAGMSG(t) => write_cmd(f, "TAGMSG", &[t]).map(|_| ()),
            Command::ACK => f.write_str("ACK"),
            Command::WEBIRC(pass, gateway, host, ip, Some(opts)) => {
                write_cmd(f, "WEBIRC", &[pass, gateway, host, ip, opts]).map(|_| ())
            }
            Command::WEBIRC(pass, gateway, host, ip, None) => {
                write_cmd(f, "WEBIRC", &[pass, gateway, host, ip]).map(|_| ())
            }
            Command::CHATHISTORY {
                subcommand,
                target,
                msg_ref1,
                msg_ref2,
                limit,
            } => {
                use crate::command::subcommands::ChatHistorySubCommand;
                f.write_str("CHATHISTORY ")?;
                write!(f, "{}", subcommand)?;
                match subcommand {
                    ChatHistorySubCommand::TARGETS => {
                        // TARGETS <timestamp> <timestamp> <limit>
                        write!(f, " {} ", msg_ref1)?;
                        if let Some(ref2) = msg_ref2 {
                            write!(f, "{} ", ref2)?;
                        }
                        write!(f, "{}", limit)
                    }
                    ChatHistorySubCommand::BETWEEN => {
                        // BETWEEN <target> <msgref> <msgref> <limit>
                        write!(f, " {} {} ", target, msg_ref1)?;
                        if let Some(ref2) = msg_ref2 {
                            write!(f, "{} ", ref2)?;
                        }
                        write!(f, "{}", limit)
                    }
                    _ => {
                        // LATEST/BEFORE/AFTER/AROUND <target> <msgref> <limit>
                        write!(f, " {} {} {}", target, msg_ref1, limit)
                    }
                }
            }
            Command::ChatHistoryTargets { target, timestamp } => {
                f.write_str("CHATHISTORY TARGETS ")?;
                write!(f, "{} {}", target, timestamp)
            }
            Command::NPC {
                channel,
                nick,
                text,
            } => write_cmd_freeform(f, "NPC", &[channel, nick, text]).map(|_| ()),
            Command::RELAYMSG {
                relay_from,
                target,
                text,
            } => write_cmd_freeform(f, "RELAYMSG", &[relay_from, target, text]).map(|_| ()),
            Command::FAIL(command, code, context) => {
                write_standard_reply(f, "FAIL", command.as_str(), code.as_str(), context)
                    .map(|_| ())
            }
            Command::WARN(command, code, context) => {
                write_standard_reply(f, "WARN", command.as_str(), code.as_str(), context)
                    .map(|_| ())
            }
            Command::NOTE(command, code, context) => {
                write_standard_reply(f, "NOTE", command.as_str(), code.as_str(), context)
                    .map(|_| ())
            }
            Command::REGISTER { account, message } => {
                f.write_str("REGISTER SUCCESS ")?;
                f.write_str(account)?;
                if let Some(msg) = message {
                    f.write_str(" :")?;
                    f.write_str(msg)?;
                }
                Ok(())
            }
            Command::Response(resp, a) => {
                // Write the 3-digit response code directly
                let code = *resp as u16;
                write!(f, "{:03}", code)?;
                for arg in a.iter().take(a.len().saturating_sub(1)) {
                    f.write_char(' ')?;
                    f.write_str(arg)?;
                }
                if let Some(last) = a.last() {
                    f.write_char(' ')?;
                    if needs_colon_prefix(last) {
                        f.write_char(':')?;
                    }
                    f.write_str(last)?;
                }
                Ok(())
            }
            Command::Raw(c, a) => {
                f.write_str(c)?;
                write_args_with_trailing(f, a.iter().map(String::as_str)).map(|_| ())
            }
        }
    }
}
