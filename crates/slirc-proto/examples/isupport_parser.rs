//! ISUPPORT parsing and mode disambiguation example
//!
//! This example demonstrates how to:
//! - Parse RPL_ISUPPORT (005) messages from the server
//! - Use PrefixSpec to disambiguate mode characters like 'q'
//! - Extract server capabilities and limits
//!
//! The 'q' mode is a classic example of IRC ecosystem fragmentation:
//! - On Libera Chat / InspIRCd: 'q' = Quiet (list mode, silences users)
//! - On UnrealIRCd / DALnet: 'q' = Founder/Owner (prefix mode like ~nick)
//!
//! The correct way to handle this is to check ISUPPORT at connection time.

use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

use slirc_proto::{
    isupport::{ChanModes, Isupport, PrefixSpec},
    Command, Message, Transport,
};

/// Stores parsed ISUPPORT information for a connection
#[derive(Debug, Default)]
struct ServerCapabilities {
    network: Option<String>,
    casemapping: Option<String>,
    chantypes: Option<String>,
    prefix: Option<PrefixInfo>,
    chanmodes: Option<ChanModesInfo>,
    nicklen: Option<usize>,
    topiclen: Option<usize>,
    has_excepts: bool,
    has_invex: bool,
}

#[derive(Debug, Clone)]
struct PrefixInfo {
    raw: String,
    modes: String,
    prefixes: String,
}

#[derive(Debug, Clone)]
struct ChanModesInfo {
    list_modes: String,      // Type A: always have a parameter (ban, except, invex)
    param_set_unset: String, // Type B: parameter when setting and unsetting (key)
    param_set_only: String,  // Type C: parameter only when setting (limit)
    no_param: String,        // Type D: never have a parameter (secret, moderated)
}

impl ServerCapabilities {
    fn parse_isupport(&mut self, isupport: &Isupport<'_>) {
        // Network name
        if let Some(network) = isupport.network() {
            self.network = Some(network.to_string());
        }

        // Case mapping (ascii, rfc1459, strict-rfc1459)
        if let Some(casemap) = isupport.casemapping() {
            self.casemapping = Some(casemap.to_string());
        }

        // Channel types (usually #&)
        if let Some(types) = isupport.chantypes() {
            self.chantypes = Some(types.to_string());
        }

        // PREFIX - this is crucial for mode disambiguation!
        if let Some(prefix_spec) = isupport.prefix() {
            self.prefix = Some(PrefixInfo {
                raw: format!("({}){}", prefix_spec.modes, prefix_spec.prefixes),
                modes: prefix_spec.modes.to_string(),
                prefixes: prefix_spec.prefixes.to_string(),
            });
        }

        // CHANMODES - categorizes channel modes by type
        if let Some(chanmodes) = isupport.chanmodes() {
            self.chanmodes = Some(ChanModesInfo {
                list_modes: chanmodes.a.to_string(),
                param_set_unset: chanmodes.b.to_string(),
                param_set_only: chanmodes.c.to_string(),
                no_param: chanmodes.d.to_string(),
            });
        }

        // Various limits
        if let Some(Some(nicklen)) = isupport.get("NICKLEN") {
            self.nicklen = nicklen.parse().ok();
        }
        if let Some(Some(topiclen)) = isupport.get("TOPICLEN") {
            self.topiclen = topiclen.parse().ok();
        }

        // Ban exceptions and invite exceptions
        self.has_excepts = isupport.has_excepts();
        self.has_invex = isupport.has_invex();
    }

    /// Check if a mode character is a prefix mode (like +o, +v, +q for founder)
    fn is_prefix_mode(&self, mode: char) -> bool {
        self.prefix
            .as_ref()
            .map(|p| p.modes.contains(mode))
            .unwrap_or(false)
    }

    /// Check if 'q' means Quiet (list mode) or Founder (prefix mode)
    fn interpret_q_mode(&self) -> &'static str {
        if self.is_prefix_mode('q') {
            "Founder/Owner (prefix mode with ~ symbol)"
        } else if let Some(ref cm) = self.chanmodes {
            if cm.list_modes.contains('q') {
                "Quiet (list mode, silences matching users)"
            } else {
                "Unknown (not in PREFIX or CHANMODES)"
            }
        } else {
            "Unknown (no CHANMODES information)"
        }
    }

    fn print_summary(&self) {
        println!("\n=== Server Capabilities ===");

        if let Some(ref network) = self.network {
            println!("Network: {}", network);
        }

        if let Some(ref casemap) = self.casemapping {
            println!("Case mapping: {}", casemap);
        }

        if let Some(ref types) = self.chantypes {
            println!("Channel types: {}", types);
        }

        if let Some(ref prefix) = self.prefix {
            println!("\nPREFIX: {}", prefix.raw);
            println!("  Modes: {} -> Prefixes: {}", prefix.modes, prefix.prefixes);
            for (mode, prefix_char) in prefix.modes.chars().zip(prefix.prefixes.chars()) {
                let name = match mode {
                    'q' => "founder/owner",
                    'a' => "admin/protected",
                    'o' => "operator",
                    'h' => "halfop",
                    'v' => "voice",
                    _ => "unknown",
                };
                println!("    +{} = {} ({})", mode, prefix_char, name);
            }
        }

        if let Some(ref cm) = self.chanmodes {
            println!("\nCHANMODES:");
            println!("  Type A (list modes): {}", cm.list_modes);
            println!("  Type B (param set/unset): {}", cm.param_set_unset);
            println!("  Type C (param set only): {}", cm.param_set_only);
            println!("  Type D (no param): {}", cm.no_param);
        }

        println!("\nLimits:");
        if let Some(n) = self.nicklen {
            println!("  NICKLEN: {}", n);
        }
        if let Some(n) = self.topiclen {
            println!("  TOPICLEN: {}", n);
        }

        println!("\nFeatures:");
        println!("  Ban exceptions (EXCEPTS): {}", self.has_excepts);
        println!("  Invite exceptions (INVEX): {}", self.has_invex);

        println!("\n=== Mode Disambiguation ===");
        println!(
            "The 'q' mode on this server means: {}",
            self.interpret_q_mode()
        );
    }
}

async fn connect_and_parse_isupport(
    server: &str,
    nick: &str,
) -> Result<ServerCapabilities, Box<dyn std::error::Error>> {
    println!("Connecting to {}...", server);

    let stream = TcpStream::connect(server).await?;
    let mut transport = Transport::tcp(stream)?;

    // Send registration
    transport
        .write_message(&Message::from(Command::NICK(nick.to_string())))
        .await?;
    transport
        .write_message(&Message::from(Command::USER(
            "isupport".to_string(),
            "0".to_string(),
            "ISUPPORT Parser Example".to_string(),
        )))
        .await?;

    let mut caps = ServerCapabilities::default();
    let mut registered = false;

    // Read until we get RPL_ENDOFMOTD or RPL_NOMOTD
    loop {
        match timeout(Duration::from_secs(30), transport.read_message()).await {
            Ok(Ok(Some(message))) => {
                match &message.command {
                    Command::PING(server, _) => {
                        transport
                            .write_message(&Message::from(Command::PONG(server.clone(), None)))
                            .await?;
                    }
                    Command::Response(response, args) => {
                        let code = response.code();

                        // RPL_ISUPPORT (005)
                        if code == 5 {
                            // Parse ISUPPORT from the response
                            let borrowed: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                            if let Some(isupport) = Isupport::from_response_args(&borrowed) {
                                caps.parse_isupport(&isupport);
                                println!("← Parsed ISUPPORT: {} tokens", isupport.iter().count());
                            }
                        }
                        // RPL_WELCOME (001)
                        else if code == 1 {
                            registered = true;
                            println!("← Registered successfully");
                        }
                        // RPL_ENDOFMOTD (376) or ERR_NOMOTD (422)
                        else if code == 376 || code == 422 {
                            println!("← End of MOTD");
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Ok(None)) => {
                return Err("Connection closed unexpectedly".into());
            }
            Ok(Err(e)) => {
                return Err(format!("Transport error: {:?}", e).into());
            }
            Err(_) => {
                return Err("Timeout waiting for server response".into());
            }
        }
    }

    if !registered {
        return Err("Failed to register".into());
    }

    // Disconnect cleanly
    transport
        .write_message(&Message::from(Command::QUIT(Some(
            "ISUPPORT example done".to_string(),
        ))))
        .await?;

    Ok(caps)
}

/// Demonstrate parsing ISUPPORT from a raw string (for testing)
fn demo_isupport_parsing() {
    println!("=== Demo: Parsing ISUPPORT strings ===\n");

    // Example ISUPPORT from Libera Chat (uses 'q' for quiet)
    let libera_isupport = "CHANMODES=eIbq,k,flj,CFLMPQScgimnprstuz CHANTYPES=# PREFIX=(ov)@+";
    println!("Libera Chat style:");
    println!("  Raw: {}", libera_isupport);

    if let Some(prefix) = PrefixSpec::parse("(ov)@+") {
        println!("  PREFIX modes: {}", prefix.modes);
        println!("  is_prefix_mode('q'): {}", prefix.is_prefix_mode('q'));
        println!("  → 'q' is NOT a prefix mode, so it's Quiet (list mode)");
    }

    if let Some(chanmodes) = ChanModes::parse("eIbq,k,flj,CFLMPQScgimnprstuz") {
        println!("  CHANMODES type A (list): {}", chanmodes.a);
        println!("  → 'q' is in type A, confirming it's a list mode (Quiet)");
    }

    println!();

    // Example ISUPPORT from UnrealIRCd (uses 'q' for founder)
    let unreal_isupport = "CHANMODES=beI,kLf,l,psmntirzMQNRTOVKDdGPZSCc PREFIX=(qaohv)~&@%+";
    println!("UnrealIRCd style:");
    println!("  Raw: {}", unreal_isupport);

    if let Some(prefix) = PrefixSpec::parse("(qaohv)~&@%+") {
        println!("  PREFIX modes: {}", prefix.modes);
        println!("  is_prefix_mode('q'): {}", prefix.is_prefix_mode('q'));
        println!("  prefix_for_mode('q'): {:?}", prefix.prefix_for_mode('q'));
        println!("  → 'q' IS a prefix mode with ~ symbol (Founder)");
    }

    println!();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // First, demonstrate parsing without a network connection
    demo_isupport_parsing();

    // Then connect to a real server
    println!("=== Live: Connecting to IRC server ===\n");

    // You can change this to any IRC server
    let server = "irc.libera.chat:6667";
    let nick = "isupport_test";

    match connect_and_parse_isupport(server, nick).await {
        Ok(caps) => {
            caps.print_summary();
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!("\nTo test without network, the demo_isupport_parsing() output above");
            eprintln!("shows how to parse ISUPPORT strings directly.");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_spec_disambiguation() {
        // Libera Chat style - q is NOT a prefix mode
        let libera = PrefixSpec::parse("(ov)@+").unwrap();
        assert!(!libera.is_prefix_mode('q'));
        assert!(libera.is_prefix_mode('o'));
        assert!(libera.is_prefix_mode('v'));

        // UnrealIRCd style - q IS a prefix mode (founder)
        let unreal = PrefixSpec::parse("(qaohv)~&@%+").unwrap();
        assert!(unreal.is_prefix_mode('q'));
        assert_eq!(unreal.prefix_for_mode('q'), Some('~'));
        assert_eq!(unreal.mode_for_prefix('~'), Some('q'));
    }

    #[test]
    fn test_chanmodes_parsing() {
        let cm = ChanModes::parse("eIbq,k,flj,CFLMPQScgimnprstuz").unwrap();
        assert!(cm.a.contains('q')); // q is in list modes on Libera
        assert!(cm.a.contains('b')); // ban
        assert!(cm.b.contains('k')); // key
        assert!(cm.c.contains('l')); // limit
    }
}
