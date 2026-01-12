use super::super::types::Command;
use super::connection::raw;
use crate::error::MessageParseError;

pub(super) fn parse(cmd: &str, args: Vec<&str>) -> Result<Command, MessageParseError> {
    let result = match cmd {
        "WHO" => {
            if args.is_empty() {
                Command::WHO(None, None)
            } else if args.len() == 1 {
                Command::WHO(Some(args[0].to_owned()), None)
            } else if args.len() == 2 {
                // Preserve full second argument for WHOX support (%fields or "o")
                Command::WHO(Some(args[0].to_owned()), Some(args[1].to_owned()))
            } else {
                raw(cmd, args)
            }
        }
        "WHOIS" => {
            if args.len() == 1 {
                Command::WHOIS(None, args[0].to_owned())
            } else if args.len() == 2 {
                Command::WHOIS(Some(args[0].to_owned()), args[1].to_owned())
            } else {
                raw(cmd, args)
            }
        }
        "WHOWAS" => {
            if args.len() == 1 {
                Command::WHOWAS(args[0].to_owned(), None, None)
            } else if args.len() == 2 {
                Command::WHOWAS(args[0].to_owned(), None, Some(args[1].to_owned()))
            } else if args.len() == 3 {
                Command::WHOWAS(
                    args[0].to_owned(),
                    Some(args[1].to_owned()),
                    Some(args[2].to_owned()),
                )
            } else {
                raw(cmd, args)
            }
        }
        _ => unreachable!("user::parse called with non-user command: {}", cmd),
    };

    Ok(result)
}
