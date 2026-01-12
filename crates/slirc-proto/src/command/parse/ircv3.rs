use super::super::types::Command;
use super::connection::raw;
use crate::error::MessageParseError;

pub(super) fn parse(cmd: &str, args: Vec<&str>) -> Result<Command, MessageParseError> {
    let result = match cmd {
        "CAP" => {
            if args.len() == 1 {
                match args[0].parse() {
                    Ok(c) => Command::CAP(None, c, None, None),
                    Err(_) => raw(cmd, args),
                }
            } else if args.len() == 2 {
                match args[0].parse() {
                    Ok(c) => Command::CAP(None, c, Some(args[1].to_owned()), None),
                    Err(_) => raw(cmd, args),
                }
            } else if args.len() == 3 {
                if let Ok(cmd_parsed) = args[1].parse() {
                    Command::CAP(
                        Some(args[0].to_owned()),
                        cmd_parsed,
                        Some(args[2].to_owned()),
                        None,
                    )
                } else {
                    raw(cmd, args)
                }
            } else if args.len() == 4 {
                if let Ok(cmd_parsed) = args[1].parse() {
                    Command::CAP(
                        Some(args[0].to_owned()),
                        cmd_parsed,
                        Some(args[2].to_owned()),
                        Some(args[3].to_owned()),
                    )
                } else {
                    raw(cmd, args)
                }
            } else {
                raw(cmd, args)
            }
        }
        "AUTHENTICATE" => {
            if args.len() == 1 {
                Command::AUTHENTICATE(args[0].to_owned())
            } else {
                raw(cmd, args)
            }
        }
        "ACCOUNT" => {
            if args.len() == 1 {
                Command::ACCOUNT(args[0].to_owned())
            } else {
                raw(cmd, args)
            }
        }
        "MONITOR" => {
            if args.len() == 2 {
                Command::MONITOR(args[0].to_owned(), Some(args[1].to_owned()))
            } else if args.len() == 1 {
                Command::MONITOR(args[0].to_owned(), None)
            } else {
                raw(cmd, args)
            }
        }
        "BATCH" => {
            if args.len() == 1 {
                Command::BATCH(args[0].to_owned(), None, None)
            } else if args.len() == 2 {
                match args[1].parse() {
                    Ok(sub) => Command::BATCH(args[0].to_owned(), Some(sub), None),
                    Err(_) => raw(cmd, args),
                }
            } else if args.len() > 2 {
                match args[1].parse() {
                    Ok(sub) => Command::BATCH(
                        args[0].to_owned(),
                        Some(sub),
                        Some(args.iter().skip(2).map(|s| s.to_string()).collect()),
                    ),
                    Err(_) => raw(cmd, args),
                }
            } else {
                raw(cmd, args)
            }
        }
        "CHGHOST" => {
            if args.len() == 2 {
                Command::CHGHOST(args[0].to_owned(), args[1].to_owned())
            } else {
                raw(cmd, args)
            }
        }
        "CHGIDENT" => {
            if args.len() == 2 {
                Command::CHGIDENT(args[0].to_owned(), args[1].to_owned())
            } else {
                raw(cmd, args)
            }
        }
        "SETNAME" => {
            if args.len() == 1 {
                Command::SETNAME(args[0].to_owned())
            } else {
                raw(cmd, args)
            }
        }
        "TAGMSG" => {
            if args.len() == 1 {
                Command::TAGMSG(args[0].to_owned())
            } else {
                raw(cmd, args)
            }
        }
        "ACK" => {
            // ACK takes no parameters
            Command::ACK
        }
        "WEBIRC" => {
            if args.len() >= 4 {
                Command::WEBIRC(
                    args[0].to_owned(),
                    args[1].to_owned(),
                    args[2].to_owned(),
                    args[3].to_owned(),
                    args.get(4).map(|s| s.to_string()),
                )
            } else {
                raw(cmd, args)
            }
        }
        "CHATHISTORY" => {
            use crate::command::subcommands::{ChatHistorySubCommand, MessageReference};
            if args.len() < 3 {
                return Ok(raw(cmd, args));
            }
            let subcommand = match args[0].parse::<ChatHistorySubCommand>() {
                Ok(s) => s,
                Err(_) => return Ok(raw(cmd, args)),
            };
            match subcommand {
                ChatHistorySubCommand::TARGETS => {
                    // TARGETS <timestamp> <timestamp> <limit>
                    if args.len() < 4 {
                        return Ok(raw(cmd, args));
                    }
                    let msg_ref1 = MessageReference::parse(args[1])
                        .unwrap_or(MessageReference::Timestamp(args[1].to_owned()));
                    let msg_ref2 = MessageReference::parse(args[2]).ok();
                    let limit = args[3].parse::<u32>().unwrap_or(50);
                    Command::CHATHISTORY {
                        subcommand,
                        target: String::new(), // TARGETS has no target
                        msg_ref1,
                        msg_ref2,
                        limit,
                    }
                }
                ChatHistorySubCommand::BETWEEN => {
                    // BETWEEN <target> <msgref> <msgref> <limit>
                    if args.len() < 5 {
                        return Ok(raw(cmd, args));
                    }
                    let target = args[1].to_owned();
                    let msg_ref1 = match MessageReference::parse(args[2]) {
                        Ok(r) => r,
                        Err(_) => return Ok(raw(cmd, args)),
                    };
                    let msg_ref2 = MessageReference::parse(args[3]).ok();
                    let limit = args[4].parse::<u32>().unwrap_or(50);
                    Command::CHATHISTORY {
                        subcommand,
                        target,
                        msg_ref1,
                        msg_ref2,
                        limit,
                    }
                }
                _ => {
                    // LATEST/BEFORE/AFTER/AROUND <target> <msgref> <limit>
                    if args.len() < 4 {
                        return Ok(raw(cmd, args));
                    }
                    let target = args[1].to_owned();
                    let msg_ref1 = match MessageReference::parse(args[2]) {
                        Ok(r) => r,
                        Err(_) => return Ok(raw(cmd, args)),
                    };
                    let limit = args[3].parse::<u32>().unwrap_or(50);
                    Command::CHATHISTORY {
                        subcommand,
                        target,
                        msg_ref1,
                        msg_ref2: None,
                        limit,
                    }
                }
            }
        }
        _ => raw(cmd, args),
    };

    Ok(result)
}
