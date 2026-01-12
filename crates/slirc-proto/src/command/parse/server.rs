use super::super::types::Command;
use super::connection::raw;
use crate::error::MessageParseError;

pub(super) fn parse(cmd: &str, args: Vec<&str>) -> Result<Command, MessageParseError> {
    let result = match cmd {
        "MOTD" => {
            if args.is_empty() {
                Command::MOTD(None)
            } else if args.len() == 1 {
                Command::MOTD(Some(args[0].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "LUSERS" => {
            if args.is_empty() {
                Command::LUSERS(None, None)
            } else if args.len() == 1 {
                Command::LUSERS(Some(args[0].to_owned()), None)
            } else if args.len() == 2 {
                Command::LUSERS(Some(args[0].to_owned()), Some(args[1].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "VERSION" => {
            if args.is_empty() {
                Command::VERSION(None)
            } else if args.len() == 1 {
                Command::VERSION(Some(args[0].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "STATS" => {
            if args.is_empty() {
                Command::STATS(None, None)
            } else if args.len() == 1 {
                Command::STATS(Some(args[0].to_owned()), None)
            } else if args.len() == 2 {
                Command::STATS(Some(args[0].to_owned()), Some(args[1].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "LINKS" => {
            if args.is_empty() {
                Command::LINKS(None, None)
            } else if args.len() == 1 {
                Command::LINKS(Some(args[0].to_owned()), None)
            } else if args.len() == 2 {
                Command::LINKS(Some(args[0].to_owned()), Some(args[1].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "TIME" => {
            if args.is_empty() {
                Command::TIME(None)
            } else if args.len() == 1 {
                Command::TIME(Some(args[0].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "CONNECT" => {
            if args.len() != 2 {
                raw(cmd, args)
            } else {
                Command::CONNECT(args[0].to_owned(), args[1].to_owned(), None)
            }
        }
        "TRACE" => {
            if args.is_empty() {
                Command::TRACE(None)
            } else if args.len() == 1 {
                Command::TRACE(Some(args[0].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "ADMIN" => {
            if args.is_empty() {
                Command::ADMIN(None)
            } else if args.len() == 1 {
                Command::ADMIN(Some(args[0].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "INFO" => {
            if args.is_empty() {
                Command::INFO(None)
            } else if args.len() == 1 {
                Command::INFO(Some(args[0].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "SID" => {
            if args.len() == 4 {
                Command::SID(
                    args[0].to_owned(),
                    args[1].to_owned(),
                    args[2].to_owned(),
                    args[3].to_owned(),
                )
            } else {
                raw(cmd, args)
            }
        }
        "UID" => {
            if args.len() == 8 {
                Command::UID(
                    args[0].to_owned(),
                    args[1].to_owned(),
                    args[2].to_owned(),
                    args[3].to_owned(),
                    args[4].to_owned(),
                    args[5].to_owned(),
                    args[6].to_owned(),
                    args[7].to_owned(),
                )
            } else {
                raw(cmd, args)
            }
        }
        "MAP" => {
            if args.is_empty() {
                Command::MAP
            } else {
                raw(cmd, args)
            }
        }
        "RULES" => {
            if args.is_empty() {
                Command::RULES
            } else {
                raw(cmd, args)
            }
        }
        "USERIP" => Command::USERIP(args.into_iter().map(|s| s.to_owned()).collect()),
        "HELP" => {
            if args.is_empty() {
                Command::HELP(None)
            } else {
                Command::HELP(Some(args[0].to_owned()))
            }
        }
        "METADATA" => {
            if args.len() < 2 {
                raw(cmd, args)
            } else {
                let subcommand =
                    match args[0].parse::<super::super::subcommands::MetadataSubCommand>() {
                        Ok(sub) => sub,
                        Err(_) => return Ok(raw(cmd, args)),
                    };
                let target = args[1].to_owned();
                let params: Vec<String> = args[2..].iter().map(|s| s.to_string()).collect();
                Command::METADATA {
                    subcommand,
                    target,
                    params,
                }
            }
        }
        "SERVLIST" => {
            if args.is_empty() {
                Command::SERVLIST(None, None)
            } else if args.len() == 1 {
                Command::SERVLIST(Some(args[0].to_owned()), None)
            } else if args.len() == 2 {
                Command::SERVLIST(Some(args[0].to_owned()), Some(args[1].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "SQUERY" => {
            if args.len() != 2 {
                raw(cmd, args)
            } else {
                Command::SQUERY(args[0].to_owned(), args[1].to_owned())
            }
        }
        "SJOIN" => {
            // SJOIN ts channel modes [args...] :users
            if args.len() < 4 {
                raw(cmd, args)
            } else {
                let ts = args[0].parse().unwrap_or(0);
                let channel = args[1].to_owned();
                let modes = args[2].to_owned();

                // The last argument is the user list
                let user_list_str = args.last().unwrap();
                let users: Vec<(String, String)> = user_list_str
                    .split_whitespace()
                    .map(|u| {
                        // Parse prefixes (e.g., @+UID)
                        let mut prefixes = String::new();
                        let mut uid_start = 0;
                        for (i, c) in u.char_indices() {
                            if "!~&@%+".contains(c) {
                                prefixes.push(c);
                            } else {
                                uid_start = i;
                                break;
                            }
                        }
                        let uid = u[uid_start..].to_string();
                        (prefixes, uid)
                    })
                    .collect();

                // Arguments between modes and user list are mode args
                let mode_args = args[3..args.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                Command::SJOIN(ts, channel, modes, mode_args, users)
            }
        }
        "TMODE" => {
            // TMODE ts channel modes [args...]
            if args.len() < 3 {
                raw(cmd, args)
            } else {
                let ts = args[0].parse().unwrap_or(0);
                let channel = args[1].to_owned();
                let modes = args[2].to_owned();
                let mode_args = args[3..].iter().map(|s| s.to_string()).collect();

                Command::TMODE(ts, channel, modes, mode_args)
            }
        }
        "ENCAP" => {
            // ENCAP target subcommand [params...]
            if args.len() < 2 {
                raw(cmd, args)
            } else {
                let target = args[0].to_owned();
                let subcommand = args[1].to_owned();
                let params = args[2..].iter().map(|s| s.to_string()).collect();
                Command::ENCAP(target, subcommand, params)
            }
        }
        "SERVER" => {
            if args.len() != 4 {
                raw(cmd, args)
            } else {
                let hopcount = args[1].parse().unwrap_or(0);
                Command::SERVER(
                    args[0].to_owned(),
                    hopcount,
                    args[2].to_owned(),
                    args[3].to_owned(),
                )
            }
        }
        "CAPAB" => Command::CAPAB(args.iter().map(|s| s.to_string()).collect()),
        "SVINFO" => {
            if args.len() < 4 {
                raw(cmd, args)
            } else {
                let v = args[0].parse().unwrap_or(0);
                let m = args[1].parse().unwrap_or(0);
                let z = args[2].parse().unwrap_or(0);
                let t = args[3].parse().unwrap_or(0);
                Command::SVINFO(v, m, z, t)
            }
        }
        _ => unreachable!("server::parse called with non-server command: {}", cmd),
    };

    Ok(result)
}
