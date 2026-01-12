//! WebSocket IRC server example
//!
//! This example demonstrates how to create a WebSocket-based IRC server using slirc-proto.
//! It shows how to:
//! - Bind a `TcpListener` for incoming WebSocket connections
//! - Perform the WebSocket handshake using `tungstenite`
//! - Validate origin and negotiate subprotocols using `WebSocketConfig`
//! - Wrap the upgraded connection in `ZeroCopyTransportEnum::websocket()`
//! - Handle WEBIRC commands from WebSocket gateways (The Lounge, KiwiIRC, etc.)
//!
//! This is a reference implementation for the Server Team to integrate WebSocket
//! support into SLIRCd.
//!
//! Run with: `cargo run --example websocket_server`
//! Test with a WebSocket client or browser-based IRC client.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::handshake::server::Callback;
use tokio_tungstenite::tungstenite::http::Response;

use slirc_proto::websocket::{
    build_handshake_response, validate_handshake, HandshakeResult, WebSocketConfig,
};
use slirc_proto::{Command, Message, ZeroCopyTransportEnum};

/// Configuration for the WebSocket IRC server.
struct ServerConfig {
    /// Address to bind the server to.
    bind_addr: SocketAddr,
    /// WebSocket configuration (origin validation, CORS, subprotocol).
    ws_config: WebSocketConfig,
    /// WEBIRC password for gateway authentication.
    webirc_password: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:6668".parse().expect("Valid address"),
            ws_config: WebSocketConfig::development(),
            webirc_password: "secret_gateway_password".to_string(),
        }
    }
}

/// Callback handler for WebSocket handshake validation.
///
/// This is passed to `tokio_tungstenite::accept_hdr_async` to validate
/// the incoming WebSocket upgrade request.
struct WebSocketHandshakeCallback {
    config: WebSocketConfig,
    result: Option<HandshakeResult>,
}

impl WebSocketHandshakeCallback {
    fn new(config: WebSocketConfig) -> Self {
        Self {
            config,
            result: None,
        }
    }
}

impl Callback for WebSocketHandshakeCallback {
    fn on_request(
        mut self,
        request: &tokio_tungstenite::tungstenite::handshake::server::Request,
        response: Response<()>,
    ) -> Result<Response<()>, tokio_tungstenite::tungstenite::handshake::server::ErrorResponse>
    {
        // Validate the handshake using slirc-proto's WebSocket validation
        let validation_result = validate_handshake(request, &self.config);

        match &validation_result {
            HandshakeResult::Accept {
                subprotocol,
                origin,
            } => {
                println!(
                    "  ✓ Handshake accepted (origin: {:?}, protocol: {:?})",
                    origin, subprotocol
                );

                // Build response with CORS headers and subprotocol
                let custom_response = build_handshake_response(&validation_result, &self.config)?;

                // Merge headers from custom response into the tungstenite response
                let mut final_response = response;
                for (name, value) in custom_response.headers() {
                    final_response
                        .headers_mut()
                        .insert(name.clone(), value.clone());
                }

                self.result = Some(validation_result);
                Ok(final_response)
            }
            HandshakeResult::Reject { status, reason } => {
                println!("  ✗ Handshake rejected: {} - {}", status, reason);
                Err(
                    tokio_tungstenite::tungstenite::handshake::server::ErrorResponse::new(Some(
                        reason.clone(),
                    )),
                )
            }
            // Handle future HandshakeResult variants (non-exhaustive enum)
            _ => Err(
                tokio_tungstenite::tungstenite::handshake::server::ErrorResponse::new(Some(
                    "Unknown handshake result".to_string(),
                )),
            ),
        }
    }
}

/// Handle an incoming WebSocket IRC connection.
///
/// This function demonstrates the complete flow for a WebSocket IRC client:
/// 1. Accept the TCP connection
/// 2. Perform WebSocket handshake with validation
/// 3. Create a zero-copy transport for high-performance message processing
/// 4. Handle WEBIRC commands from WebSocket gateways
/// 5. Process IRC messages in a loop
async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    config: &ServerConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("\n[{}] Connection accepted", addr);

    // Create the handshake callback
    let callback = WebSocketHandshakeCallback::new(config.ws_config.clone());

    // Perform WebSocket handshake with origin validation
    let ws_stream = match tokio_tungstenite::accept_hdr_async(stream, callback).await {
        Ok(ws) => ws,
        Err(e) => {
            println!("[{}] WebSocket handshake failed: {}", addr, e);
            return Ok(());
        }
    };

    println!("[{}] WebSocket connection established", addr);

    // Wrap the WebSocket stream in a zero-copy transport
    // This is the key API for the Server Team - ZeroCopyTransportEnum::websocket()
    let mut transport = ZeroCopyTransportEnum::websocket(ws_stream);

    // Track client state
    let mut client_ip: Option<String> = None;
    let mut client_hostname: Option<String> = None;
    let mut _registered = false;

    // Process messages
    loop {
        match timeout(Duration::from_secs(300), transport.next()).await {
            Ok(Some(Ok(msg_ref))) => {
                println!(
                    "[{}] ← {} {}",
                    addr,
                    msg_ref.command.name,
                    msg_ref.command.args.join(" ")
                );

                // Convert to owned for pattern matching (in a real server you'd use msg_ref directly)
                let message: Message = msg_ref.to_owned();

                match &message.command {
                    // WEBIRC - Gateway passes real client IP
                    // Format: WEBIRC password gateway hostname ip [:options]
                    //
                    // WebSocket gateways like The Lounge or KiwiIRC send this command
                    // before NICK/USER to inform the server of the real client's IP address.
                    Command::WEBIRC(password, gateway, hostname, ip, options) => {
                        if password == &config.webirc_password {
                            client_ip = Some(ip.clone());
                            client_hostname = Some(hostname.clone());
                            println!(
                                "[{}] ✓ WEBIRC accepted from gateway '{}': {}@{}",
                                addr, gateway, ip, hostname
                            );
                            if let Some(opts) = options {
                                println!("[{}]   Options: {}", addr, opts);
                            }

                            // In a real server, you would:
                            // 1. Update the client's connection info to use the real IP
                            // 2. Log the gateway name for audit purposes
                            // 3. Apply any IP-based restrictions (K-lines, etc.) using the real IP
                        } else {
                            println!(
                                "[{}] ✗ WEBIRC rejected: invalid password from '{}'",
                                addr, gateway
                            );
                            // In a real server, you might disconnect or ignore
                        }
                    }

                    Command::NICK(nickname) => {
                        println!("[{}] Client nickname: {}", addr, nickname);
                    }

                    Command::USER(username, _, realname) => {
                        println!("[{}] Client registered: {} ({})", addr, username, realname);
                        if let Some(ref ip) = client_ip {
                            println!("[{}]   Real IP (via WEBIRC): {}", addr, ip);
                        }
                        if let Some(ref host) = client_hostname {
                            println!("[{}]   Real hostname (via WEBIRC): {}", addr, host);
                        }
                        _registered = true;

                        // Send a welcome message (simplified)
                        println!("[{}] → 001 Welcome to the WebSocket IRC server!", addr);
                    }

                    Command::PING(server, _) => {
                        println!("[{}] → PONG :{}", addr, server);
                    }

                    Command::QUIT(reason) => {
                        println!(
                            "[{}] Client quit: {}",
                            addr,
                            reason.as_deref().unwrap_or("No reason")
                        );
                        break;
                    }

                    Command::PRIVMSG(target, text) => {
                        println!("[{}] PRIVMSG {} :{}", addr, target, text);
                    }

                    _ => {
                        // Handle other commands...
                    }
                }
            }

            Ok(Some(Err(e))) => {
                println!("[{}] Parse error: {:?}", addr, e);
            }

            Ok(None) => {
                println!("[{}] Connection closed by client", addr);
                break;
            }

            Err(_) => {
                println!("[{}] Connection timeout (idle)", addr);
                break;
            }
        }
    }

    println!("[{}] Session ended\n", addr);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== WebSocket IRC Server Example ===\n");

    // Server configuration
    let config = ServerConfig::default();

    println!("Configuration:");
    println!("  Bind address: {}", config.bind_addr);
    println!("  WEBIRC password: {}", config.webirc_password);
    println!(
        "  Allowed origins: {:?}",
        if config.ws_config.allowed_origins.is_empty() {
            vec!["(any)".to_string()]
        } else {
            config.ws_config.allowed_origins.clone()
        }
    );
    println!(
        "  Subprotocol: {:?}",
        config.ws_config.subprotocol.as_deref().unwrap_or("(none)")
    );
    println!();

    // Bind the TCP listener
    let listener = TcpListener::bind(config.bind_addr).await?;
    println!("Server listening on ws://{}", config.bind_addr);
    println!();
    println!("To test, connect with a WebSocket IRC client or use wscat:");
    println!("  wscat -c ws://127.0.0.1:6668 -s irc");
    println!();
    println!("Example commands to send:");
    println!("  WEBIRC secret_gateway_password TheLounge client.example.com 192.168.1.100");
    println!("  NICK testnick");
    println!("  USER testuser 0 * :Test User");
    println!("  PRIVMSG #test :Hello, WebSocket world!");
    println!("  QUIT :Goodbye");
    println!();

    // Accept connections
    loop {
        let (stream, addr) = listener.accept().await?;

        // Clone config for the spawned task
        let ws_config = config.ws_config.clone();
        let webirc_password = config.webirc_password.clone();

        // Spawn a task to handle the connection
        tokio::spawn(async move {
            let task_config = ServerConfig {
                bind_addr: addr, // Not used, but satisfies struct
                ws_config,
                webirc_password,
            };

            if let Err(e) = handle_connection(stream, addr, &task_config).await {
                eprintln!("[{}] Error: {}", addr, e);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.bind_addr.port(), 6668);
        assert!(!config.webirc_password.is_empty());
    }

    #[test]
    fn test_webirc_parsing() {
        // Test that WEBIRC command is correctly parsed
        let raw = "WEBIRC secret TheLounge client.example.com 192.168.1.100";
        let msg: Message = raw.parse().expect("Should parse WEBIRC");

        match msg.command {
            Command::WEBIRC(pass, gateway, host, ip, opts) => {
                assert_eq!(pass, "secret");
                assert_eq!(gateway, "TheLounge");
                assert_eq!(host, "client.example.com");
                assert_eq!(ip, "192.168.1.100");
                assert!(opts.is_none());
            }
            _ => panic!("Expected WEBIRC command"),
        }
    }

    #[test]
    fn test_webirc_with_options() {
        // Test WEBIRC with optional flags
        let raw = "WEBIRC secret KiwiIRC client.host 10.0.0.1 :secure";
        let msg: Message = raw.parse().expect("Should parse WEBIRC with options");

        match msg.command {
            Command::WEBIRC(pass, gateway, host, ip, opts) => {
                assert_eq!(pass, "secret");
                assert_eq!(gateway, "KiwiIRC");
                assert_eq!(host, "client.host");
                assert_eq!(ip, "10.0.0.1");
                assert_eq!(opts, Some("secure".to_string()));
            }
            _ => panic!("Expected WEBIRC command"),
        }
    }
}
