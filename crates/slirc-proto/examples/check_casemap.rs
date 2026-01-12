use slirc_proto::irc_to_lower;

fn main() {
    println!("'Nick_2' -> '{}'", irc_to_lower("Nick_2"));
    println!("'nick_2' -> '{}'", irc_to_lower("nick_2"));
    println!("'[' -> '{}'", irc_to_lower("["));
    println!("'{{' -> '{}'", irc_to_lower("{"));
}
