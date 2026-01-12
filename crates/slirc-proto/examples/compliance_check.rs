use slirc_proto::compliance::{check_compliance, ComplianceConfig};
use slirc_proto::MessageRef;

fn main() {
    let raw = ":nick!user@host PRIVMSG #channel :Hello world!";
    let msg = MessageRef::parse(raw).unwrap();
    let config = ComplianceConfig::default();

    match check_compliance(&msg, Some(raw.len()), &config) {
        Ok(_) => println!("Message is compliant!"),
        Err(errors) => {
            println!("Message is not compliant:");
            for err in errors {
                println!("- {}", err);
            }
        }
    }

    let invalid_raw = "JOIN invalid_channel";
    let invalid_msg = MessageRef::parse(invalid_raw).unwrap();
    let strict_config = ComplianceConfig {
        strict_channel_names: true,
        ..Default::default()
    };

    println!("\nChecking invalid message with strict config:");
    match check_compliance(&invalid_msg, Some(invalid_raw.len()), &strict_config) {
        Ok(_) => println!("Message is compliant!"),
        Err(errors) => {
            println!("Message is not compliant:");
            for err in errors {
                println!("- {}", err);
            }
        }
    }
}
