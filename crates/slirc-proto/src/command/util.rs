use std::fmt;
use std::io;

use crate::mode::{Mode, ModeType};

/// A trait for abstracting over `fmt::Formatter` and `io::Write`.
/// This allows sharing serialization logic between `Display` and `IrcEncode`.
pub trait IrcSink {
    type Error;
    fn write_str(&mut self, s: &str) -> Result<usize, Self::Error>;
    fn write_char(&mut self, c: char) -> Result<usize, Self::Error>;
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<usize, Self::Error>;
    fn return_error(&self, msg: &str) -> Self::Error;
}

impl<'a> IrcSink for fmt::Formatter<'a> {
    type Error = fmt::Error;

    fn write_str(&mut self, s: &str) -> Result<usize, Self::Error> {
        fmt::Write::write_str(self, s).map(|_| 0)
    }

    fn write_char(&mut self, c: char) -> Result<usize, Self::Error> {
        fmt::Write::write_char(self, c).map(|_| 0)
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<usize, Self::Error> {
        fmt::Write::write_fmt(self, args).map(|_| 0)
    }

    fn return_error(&self, _msg: &str) -> Self::Error {
        fmt::Error
    }
}

/// Wrapper to adapt `io::Write` to `IrcSink`.
pub struct IoWriteSink<'a, W: ?Sized>(pub &'a mut W);

impl<'a, W: io::Write + ?Sized> IrcSink for IoWriteSink<'a, W> {
    type Error = io::Error;

    fn write_str(&mut self, s: &str) -> Result<usize, Self::Error> {
        self.0.write_all(s.as_bytes()).map(|_| s.len())
    }

    fn write_char(&mut self, c: char) -> Result<usize, Self::Error> {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        self.0.write_all(s.as_bytes()).map(|_| s.len())
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<usize, Self::Error> {
        // We need to count bytes, so we format to a string first.
        // This involves allocation, but it's necessary to get the length
        // since io::Write::write_fmt doesn't return it.
        let s = fmt::format(args);
        self.0.write_all(s.as_bytes()).map(|_| s.len())
    }

    fn return_error(&self, msg: &str) -> Self::Error {
        io::Error::new(io::ErrorKind::InvalidInput, msg)
    }
}

/// Check if a string needs colon-prefixing as a trailing IRC argument.
pub fn needs_colon_prefix(s: &str) -> bool {
    s.is_empty() || s.contains(' ') || s.starts_with(':')
}

/// Validate a parameter for IRC injection safety.
pub fn validate_param<S: IrcSink + ?Sized>(sink: &S, param: &str) -> Result<(), S::Error> {
    if param
        .as_bytes()
        .iter()
        .any(|&b| b == b'\r' || b == b'\n' || b == 0)
    {
        return Err(sink.return_error("Parameter contains invalid control characters"));
    }
    Ok(())
}

/// Write mode flags with collapsed signs (e.g., +ovh instead of +o+v+h).
pub fn write_collapsed_mode_flags<S: IrcSink, T: ModeType>(
    sink: &mut S,
    modes: &[Mode<T>],
) -> Result<usize, S::Error> {
    #[derive(PartialEq, Clone, Copy)]
    enum Sign {
        Plus,
        Minus,
        None,
    }

    let mut current_sign = Sign::None;
    let mut count = 0;

    for m in modes {
        let (new_sign, mode) = match m {
            Mode::Plus(mode, _) => (Sign::Plus, mode),
            Mode::Minus(mode, _) => (Sign::Minus, mode),
            Mode::NoPrefix(mode) => (Sign::None, mode),
        };

        // Only write sign when it changes
        if new_sign != current_sign {
            match new_sign {
                Sign::Plus => count += sink.write_char('+')?,
                Sign::Minus => count += sink.write_char('-')?,
                Sign::None => {}
            }
            current_sign = new_sign;
        }

        count += sink.write_fmt(format_args!("{}", mode))?;
    }

    Ok(count)
}

/// Write a standard reply (FAIL/WARN/NOTE) with command, code, and context.
/// The last context argument is always colon-prefixed (freeform).
pub fn write_standard_reply<S: IrcSink>(
    sink: &mut S,
    reply_type: &str,
    command: &str,
    code: &str,
    context: &[String],
) -> Result<usize, S::Error> {
    let mut count = 0;
    count += sink.write_str(reply_type)?;
    count += sink.write_char(' ')?;
    count += sink.write_str(command)?;
    count += sink.write_char(' ')?;
    count += sink.write_str(code)?;
    for (i, arg) in context.iter().enumerate() {
        validate_param(sink, arg)?;
        count += sink.write_char(' ')?;
        // Last argument gets colon prefix (freeform)
        if i == context.len() - 1 {
            count += sink.write_char(':')?;
        }
        count += sink.write_str(arg)?;
    }
    Ok(count)
}

/// Write a command with arguments directly to a sink.
/// The last argument is treated as trailing and gets a `:` prefix if needed.
pub fn write_cmd<S: IrcSink>(sink: &mut S, cmd: &str, args: &[&str]) -> Result<usize, S::Error> {
    if args.is_empty() {
        return sink.write_str(cmd);
    }

    let mut count = 0;
    let (middle_params, trailing) = args.split_at(args.len() - 1);
    let trailing = trailing[0];

    count += sink.write_str(cmd)?;

    for param in middle_params {
        validate_param(sink, param)?;
        count += sink.write_char(' ')?;
        count += sink.write_str(param)?;
    }

    validate_param(sink, trailing)?;
    count += sink.write_char(' ')?;

    // Add colon prefix if trailing is empty, contains a space, or starts with ':'
    if needs_colon_prefix(trailing) {
        count += sink.write_char(':')?;
    }

    count += sink.write_str(trailing)?;
    Ok(count)
}

/// Write a command with a freeform (always colon-prefixed) trailing argument.
pub fn write_cmd_freeform<S: IrcSink>(
    sink: &mut S,
    cmd: &str,
    args: &[&str],
) -> Result<usize, S::Error> {
    let mut count = 0;
    match args.split_last() {
        Some((suffix, middle)) => {
            count += sink.write_str(cmd)?;
            for arg in middle {
                validate_param(sink, arg)?;
                count += sink.write_char(' ')?;
                count += sink.write_str(arg)?;
            }
            validate_param(sink, suffix)?;
            count += sink.write_str(" :")?;
            count += sink.write_str(suffix)?;
            Ok(count)
        }
        None => sink.write_str(cmd),
    }
}

/// Write service command arguments with trailing colon prefix.
pub fn write_service_args<S: IrcSink>(sink: &mut S, args: &[String]) -> Result<usize, S::Error> {
    let mut count = 0;
    let len = args.len();
    for (i, arg) in args.iter().enumerate() {
        validate_param(sink, arg)?;
        count += sink.write_char(' ')?;
        if i == len - 1 && needs_colon_prefix(arg) {
            count += sink.write_char(':')?;
        }
        count += sink.write_str(arg)?;
    }
    Ok(count)
}

/// Write arguments from an iterator, handling the last one as trailing if needed.
pub fn write_args_with_trailing<'a, S, I>(sink: &mut S, args: I) -> Result<usize, S::Error>
where
    S: IrcSink,
    I: Iterator<Item = &'a str> + ExactSizeIterator,
{
    let mut count = 0;
    let len = args.len();
    for (i, arg) in args.enumerate() {
        validate_param(sink, arg)?;
        count += sink.write_char(' ')?;
        if i == len - 1 && needs_colon_prefix(arg) {
            count += sink.write_char(':')?;
        }
        count += sink.write_str(arg)?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for needs_colon_prefix
    #[test]
    fn test_needs_colon_empty_string() {
        assert!(needs_colon_prefix(""));
    }

    #[test]
    fn test_needs_colon_contains_space() {
        assert!(needs_colon_prefix("hello world"));
    }

    #[test]
    fn test_needs_colon_starts_with_colon() {
        assert!(needs_colon_prefix(":already has colon"));
    }

    #[test]
    fn test_no_colon_simple() {
        assert!(!needs_colon_prefix("simple"));
    }

    #[test]
    fn test_no_colon_with_special_chars() {
        assert!(!needs_colon_prefix("#channel"));
        assert!(!needs_colon_prefix("user@host"));
        assert!(!needs_colon_prefix("nick!user"));
    }

    // Tests for write_cmd using a string formatter
    struct StringSink(String);

    impl IrcSink for StringSink {
        type Error = fmt::Error;

        fn write_str(&mut self, s: &str) -> Result<usize, Self::Error> {
            self.0.push_str(s);
            Ok(s.len())
        }

        fn write_char(&mut self, c: char) -> Result<usize, Self::Error> {
            self.0.push(c);
            Ok(c.len_utf8())
        }

        fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<usize, Self::Error> {
            use std::fmt::Write;
            let before = self.0.len();
            write!(self.0, "{}", args)?;
            Ok(self.0.len() - before)
        }

        fn return_error(&self, _msg: &str) -> Self::Error {
            fmt::Error
        }
    }

    #[test]
    fn test_write_cmd_no_args() {
        let mut sink = StringSink(String::new());
        write_cmd(&mut sink, "MOTD", &[]).unwrap();
        assert_eq!(sink.0, "MOTD");
    }

    #[test]
    fn test_write_cmd_single_arg() {
        let mut sink = StringSink(String::new());
        write_cmd(&mut sink, "NICK", &["testnick"]).unwrap();
        assert_eq!(sink.0, "NICK testnick");
    }

    #[test]
    fn test_write_cmd_multiple_args() {
        let mut sink = StringSink(String::new());
        write_cmd(&mut sink, "WHOIS", &["server", "nick"]).unwrap();
        assert_eq!(sink.0, "WHOIS server nick");
    }

    #[test]
    fn test_write_cmd_trailing_with_space() {
        let mut sink = StringSink(String::new());
        write_cmd(&mut sink, "QUIT", &["Goodbye world"]).unwrap();
        assert_eq!(sink.0, "QUIT :Goodbye world");
    }

    #[test]
    fn test_write_cmd_trailing_empty() {
        let mut sink = StringSink(String::new());
        write_cmd(&mut sink, "AWAY", &[""]).unwrap();
        assert_eq!(sink.0, "AWAY :");
    }

    #[test]
    fn test_write_cmd_trailing_starts_with_colon() {
        let mut sink = StringSink(String::new());
        write_cmd(
            &mut sink,
            "PRIVMSG",
            &["#channel", ":ACTION does something"],
        )
        .unwrap();
        assert_eq!(sink.0, "PRIVMSG #channel ::ACTION does something");
    }

    // Tests for write_cmd_freeform
    #[test]
    fn test_write_cmd_freeform_no_args() {
        let mut sink = StringSink(String::new());
        write_cmd_freeform(&mut sink, "MOTD", &[]).unwrap();
        assert_eq!(sink.0, "MOTD");
    }

    #[test]
    fn test_write_cmd_freeform_single_arg() {
        let mut sink = StringSink(String::new());
        write_cmd_freeform(&mut sink, "QUIT", &["Goodbye"]).unwrap();
        assert_eq!(sink.0, "QUIT :Goodbye");
    }

    #[test]
    fn test_write_cmd_freeform_multiple_args() {
        let mut sink = StringSink(String::new());
        write_cmd_freeform(&mut sink, "PRIVMSG", &["#channel", "Hello world"]).unwrap();
        assert_eq!(sink.0, "PRIVMSG #channel :Hello world");
    }

    // Tests for write_service_args
    #[test]
    fn test_write_service_args_empty() {
        let mut sink = StringSink(String::new());
        write_service_args(&mut sink, &[]).unwrap();
        assert_eq!(sink.0, "");
    }

    #[test]
    fn test_write_service_args_single() {
        let mut sink = StringSink(String::new());
        write_service_args(&mut sink, &["arg1".to_string()]).unwrap();
        assert_eq!(sink.0, " arg1");
    }

    #[test]
    fn test_write_service_args_multiple() {
        let mut sink = StringSink(String::new());
        write_service_args(&mut sink, &["arg1".to_string(), "arg2".to_string()]).unwrap();
        assert_eq!(sink.0, " arg1 arg2");
    }

    #[test]
    fn test_write_service_args_trailing_with_space() {
        let mut sink = StringSink(String::new());
        write_service_args(&mut sink, &["arg1".to_string(), "with space".to_string()]).unwrap();
        assert_eq!(sink.0, " arg1 :with space");
    }

    // Tests for write_args_with_trailing
    #[test]
    fn test_write_args_with_trailing_simple() {
        let mut sink = StringSink(String::new());
        let args = ["arg1", "arg2"];
        write_args_with_trailing(&mut sink, args.iter().copied()).unwrap();
        assert_eq!(sink.0, " arg1 arg2");
    }

    #[test]
    fn test_write_args_with_trailing_last_needs_colon() {
        let mut sink = StringSink(String::new());
        let args = ["target", "message with space"];
        write_args_with_trailing(&mut sink, args.iter().copied()).unwrap();
        assert_eq!(sink.0, " target :message with space");
    }

    // Tests for write_standard_reply
    #[test]
    fn test_write_standard_reply() {
        let mut sink = StringSink(String::new());
        write_standard_reply(
            &mut sink,
            "FAIL",
            "COMMAND",
            "CODE",
            &["context".to_string(), "description".to_string()],
        )
        .unwrap();
        assert_eq!(sink.0, "FAIL COMMAND CODE context :description");
    }

    #[test]
    fn test_write_standard_reply_single_context() {
        let mut sink = StringSink(String::new());
        write_standard_reply(
            &mut sink,
            "NOTE",
            "CMD",
            "INFO",
            &["informational message".to_string()],
        )
        .unwrap();
        assert_eq!(sink.0, "NOTE CMD INFO :informational message");
    }

    // Tests for write_collapsed_mode_flags
    #[test]
    fn test_write_collapsed_mode_flags_plus() {
        use crate::mode::ChannelMode;

        let mut sink = StringSink(String::new());
        let modes = vec![
            Mode::Plus(ChannelMode::Oper, Some("nick1".to_string())),
            Mode::Plus(ChannelMode::Voice, Some("nick2".to_string())),
        ];
        write_collapsed_mode_flags(&mut sink, &modes).unwrap();
        assert_eq!(sink.0, "+ov");
    }

    #[test]
    fn test_write_collapsed_mode_flags_minus() {
        use crate::mode::ChannelMode;

        let mut sink = StringSink(String::new());
        let modes = vec![
            Mode::Minus(ChannelMode::Oper, Some("nick1".to_string())),
            Mode::Minus(ChannelMode::Voice, Some("nick2".to_string())),
        ];
        write_collapsed_mode_flags(&mut sink, &modes).unwrap();
        assert_eq!(sink.0, "-ov");
    }

    #[test]
    fn test_write_collapsed_mode_flags_mixed() {
        use crate::mode::ChannelMode;

        let mut sink = StringSink(String::new());
        let modes = vec![
            Mode::Plus(ChannelMode::Oper, Some("nick1".to_string())),
            Mode::Minus(ChannelMode::Voice, Some("nick2".to_string())),
            Mode::Plus(ChannelMode::Halfop, Some("nick3".to_string())),
        ];
        write_collapsed_mode_flags(&mut sink, &modes).unwrap();
        assert_eq!(sink.0, "+o-v+h");
    }

    #[test]
    fn test_write_cmd_injection_vulnerability() {
        let mut sink = StringSink(String::new());
        let result = write_cmd(&mut sink, "PRIVMSG", &["#channel", "Hello\r\nQUIT"]);
        assert!(result.is_err());
    }
}
