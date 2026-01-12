use slirc_proto::ircv3::msgid::generate_msgid;
use slirc_proto::ircv3::server_time::format_server_time;
use slirc_proto::Message;

fn main() {
    let msg = Message::join("#test")
        .with_tag("msgid", Some(generate_msgid()))
        .with_tag("time", Some(format_server_time()));

    println!("{}", msg);
}
