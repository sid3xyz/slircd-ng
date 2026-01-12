use super::super::types::Command;
use super::connection::raw;
use crate::error::MessageParseError;

pub(super) fn parse(cmd: &str, args: Vec<&str>) -> Result<Command, MessageParseError> {
    let result = match cmd {
        "PRIVMSG" => {
            if args.len() != 2 {
                raw(cmd, args)
            } else {
                Command::PRIVMSG(args[0].to_owned(), args[1].to_owned())
            }
        }
        "NOTICE" => {
            if args.len() != 2 {
                raw(cmd, args)
            } else {
                Command::NOTICE(args[0].to_owned(), args[1].to_owned())
            }
        }
        "ACCEPT" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::ACCEPT(args[0].to_owned())
            }
        }

        "KILL" => {
            if args.len() != 2 {
                raw(cmd, args)
            } else {
                Command::KILL(args[0].to_owned(), args[1].to_owned())
            }
        }
        "PING" => {
            if args.len() == 1 {
                Command::PING(args[0].to_owned(), None)
            } else if args.len() == 2 {
                Command::PING(args[0].to_owned(), Some(args[1].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "PONG" => {
            if args.len() == 1 {
                Command::PONG(args[0].to_owned(), None)
            } else if args.len() == 2 {
                Command::PONG(args[0].to_owned(), Some(args[1].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "ERROR" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::ERROR(args[0].to_owned())
            }
        }

        "AWAY" => {
            if args.is_empty() {
                Command::AWAY(None)
            } else if args.len() == 1 {
                Command::AWAY(Some(args[0].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "REHASH" => {
            if args.is_empty() {
                Command::REHASH
            } else {
                raw(cmd, args)
            }
        }
        "DIE" => {
            if args.is_empty() {
                Command::DIE
            } else {
                raw(cmd, args)
            }
        }
        "RESTART" => {
            if args.is_empty() {
                Command::RESTART
            } else {
                raw(cmd, args)
            }
        }
        "SUMMON" => {
            if args.len() == 1 {
                Command::SUMMON(args[0].to_owned(), None, None)
            } else if args.len() == 2 {
                Command::SUMMON(args[0].to_owned(), Some(args[1].to_owned()), None)
            } else if args.len() == 3 {
                Command::SUMMON(
                    args[0].to_owned(),
                    Some(args[1].to_owned()),
                    Some(args[2].to_owned()),
                )
            } else {
                raw(cmd, args)
            }
        }
        "USERS" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::USERS(Some(args[0].to_owned()))
            }
        }
        "WALLOPS" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::WALLOPS(args[0].to_owned())
            }
        }
        "GLOBOPS" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::GLOBOPS(args[0].to_owned())
            }
        }
        "USERHOST" => Command::USERHOST(args.into_iter().map(|s| s.to_owned()).collect()),
        "ISON" => Command::ISON(args.into_iter().map(|s| s.to_owned()).collect()),

        "SAJOIN" => {
            if args.len() != 2 {
                raw(cmd, args)
            } else {
                Command::SAJOIN(args[0].to_owned(), args[1].to_owned())
            }
        }
        "SAMODE" => {
            if args.len() == 2 {
                Command::SAMODE(args[0].to_owned(), args[1].to_owned(), None)
            } else if args.len() == 3 {
                Command::SAMODE(
                    args[0].to_owned(),
                    args[1].to_owned(),
                    Some(args[2].to_owned()),
                )
            } else {
                raw(cmd, args)
            }
        }
        "SANICK" => {
            if args.len() != 2 {
                raw(cmd, args)
            } else {
                Command::SANICK(args[0].to_owned(), args[1].to_owned())
            }
        }
        "SAPART" => {
            if args.len() != 2 {
                raw(cmd, args)
            } else {
                Command::SAPART(args[0].to_owned(), args[1].to_owned())
            }
        }
        "SAQUIT" => {
            if args.len() != 2 {
                raw(cmd, args)
            } else {
                Command::SAQUIT(args[0].to_owned(), args[1].to_owned())
            }
        }

        // KLINE [time] user@host :reason
        // With 2 args: mask, reason (no time)
        // With 3 args: time, mask, reason
        "KLINE" => {
            if args.len() == 2 {
                Command::KLINE(None, args[0].to_owned(), args[1].to_owned())
            } else if args.len() == 3 {
                Command::KLINE(
                    Some(args[0].to_owned()),
                    args[1].to_owned(),
                    args[2].to_owned(),
                )
            } else {
                raw(cmd, args)
            }
        }

        // DLINE [time] host :reason
        // With 2 args: host, reason (no time)
        // With 3 args: time, host, reason
        "DLINE" => {
            if args.len() == 2 {
                Command::DLINE(None, args[0].to_owned(), args[1].to_owned())
            } else if args.len() == 3 {
                Command::DLINE(
                    Some(args[0].to_owned()),
                    args[1].to_owned(),
                    args[2].to_owned(),
                )
            } else {
                raw(cmd, args)
            }
        }

        // UNKLINE user@host
        "UNKLINE" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::UNKLINE(args[0].to_owned())
            }
        }

        // UNDLINE host
        "UNDLINE" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::UNDLINE(args[0].to_owned())
            }
        }

        // KNOCK channel [:message]
        "KNOCK" => {
            if args.is_empty() || args.len() > 2 {
                raw(cmd, args)
            } else if args.len() == 1 {
                Command::KNOCK(args[0].to_owned(), None)
            } else {
                Command::KNOCK(args[0].to_owned(), Some(args[1].to_owned()))
            }
        }

        // GLINE mask [reason] - Global K-line
        "GLINE" => {
            if args.is_empty() || args.len() > 2 {
                raw(cmd, args)
            } else if args.len() == 1 {
                Command::GLINE(args[0].to_owned(), None)
            } else {
                Command::GLINE(args[0].to_owned(), Some(args[1].to_owned()))
            }
        }

        // UNGLINE mask - Remove global K-line
        "UNGLINE" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::UNGLINE(args[0].to_owned())
            }
        }

        // ZLINE ip [reason] - Global IP ban
        "ZLINE" => {
            if args.is_empty() || args.len() > 2 {
                raw(cmd, args)
            } else if args.len() == 1 {
                Command::ZLINE(args[0].to_owned(), None)
            } else {
                Command::ZLINE(args[0].to_owned(), Some(args[1].to_owned()))
            }
        }

        // UNZLINE ip - Remove global IP ban
        "UNZLINE" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::UNZLINE(args[0].to_owned())
            }
        }

        // RLINE pattern [reason] - Realname/GECOS ban
        "RLINE" => {
            if args.is_empty() || args.len() > 2 {
                raw(cmd, args)
            } else if args.len() == 1 {
                Command::RLINE(args[0].to_owned(), None)
            } else {
                Command::RLINE(args[0].to_owned(), Some(args[1].to_owned()))
            }
        }

        // UNRLINE pattern - Remove realname ban
        "UNRLINE" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::UNRLINE(args[0].to_owned())
            }
        }

        // SHUN mask [reason] - Silent ignore
        "SHUN" => {
            if args.is_empty() || args.len() > 2 {
                raw(cmd, args)
            } else if args.len() == 1 {
                Command::SHUN(args[0].to_owned(), None)
            } else {
                Command::SHUN(args[0].to_owned(), Some(args[1].to_owned()))
            }
        }

        // UNSHUN mask - Remove shun
        "UNSHUN" => {
            if args.len() != 1 {
                raw(cmd, args)
            } else {
                Command::UNSHUN(args[0].to_owned())
            }
        }

        "NICKSERV" => Command::NICKSERV(args.into_iter().map(|s| s.to_owned()).collect()),
        "CHANSERV" => Command::CHANSERV(args.into_iter().map(|s| s.to_owned()).collect()),
        "OPERSERV" => Command::OPERSERV(args.into_iter().map(|s| s.to_owned()).collect()),
        "BOTSERV" => Command::BOTSERV(args.into_iter().map(|s| s.to_owned()).collect()),
        "HOSTSERV" => Command::HOSTSERV(args.into_iter().map(|s| s.to_owned()).collect()),
        "MEMOSERV" => Command::MEMOSERV(args.into_iter().map(|s| s.to_owned()).collect()),
        // Service aliases
        "NS" => Command::NS(args.into_iter().map(|s| s.to_owned()).collect()),
        "CS" => Command::CS(args.into_iter().map(|s| s.to_owned()).collect()),
        "OS" => Command::OS(args.into_iter().map(|s| s.to_owned()).collect()),
        "BS" => Command::BS(args.into_iter().map(|s| s.to_owned()).collect()),
        "HS" => Command::HS(args.into_iter().map(|s| s.to_owned()).collect()),
        "MS" => Command::MS(args.into_iter().map(|s| s.to_owned()).collect()),

        "NPC" => {
            if args.len() != 3 {
                raw(cmd, args)
            } else {
                Command::NPC {
                    channel: args[0].to_owned(),
                    nick: args[1].to_owned(),
                    text: args[2].to_owned(),
                }
            }
        }

        "RELAYMSG" => {
            if args.len() != 3 {
                raw(cmd, args)
            } else {
                Command::RELAYMSG {
                    relay_from: args[1].to_owned(),
                    target: args[0].to_owned(),
                    text: args[2].to_owned(),
                }
            }
        }

        _ => unreachable!(
            "messaging::parse called with non-messaging command: {}",
            cmd
        ),
    };

    Ok(result)
}
