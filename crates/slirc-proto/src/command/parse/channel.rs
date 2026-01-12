use super::super::types::Command;
use super::connection::raw;
use crate::error::MessageParseError;

/// Get an optional owned string from args at the given index.
#[inline]
fn arg_opt(args: &[&str], idx: usize) -> Option<String> {
    args.get(idx).map(|s| (*s).to_owned())
}

pub(super) fn parse(cmd: &str, args: Vec<&str>) -> Result<Command, MessageParseError> {
    let result = match cmd {
        "JOIN" => match args.len() {
            1..=3 => Command::JOIN(args[0].to_owned(), arg_opt(&args, 1), arg_opt(&args, 2)),
            _ => raw(cmd, args),
        },
        "PART" => match args.len() {
            1 | 2 => Command::PART(args[0].to_owned(), arg_opt(&args, 1)),
            _ => raw(cmd, args),
        },
        "TOPIC" => match args.len() {
            1 | 2 => Command::TOPIC(args[0].to_owned(), arg_opt(&args, 1)),
            _ => raw(cmd, args),
        },
        "NAMES" => match args.len() {
            0..=2 => Command::NAMES(arg_opt(&args, 0), arg_opt(&args, 1)),
            _ => raw(cmd, args),
        },
        "LIST" => match args.len() {
            0..=2 => Command::LIST(arg_opt(&args, 0), arg_opt(&args, 1)),
            _ => raw(cmd, args),
        },
        "INVITE" => match args.len() {
            2 => Command::INVITE(args[0].to_owned(), args[1].to_owned()),
            _ => raw(cmd, args),
        },
        "KICK" => match args.len() {
            2 | 3 => Command::KICK(args[0].to_owned(), args[1].to_owned(), arg_opt(&args, 2)),
            _ => raw(cmd, args),
        },
        _ => unreachable!("channel::parse called with non-channel command: {}", cmd),
    };

    Ok(result)
}
