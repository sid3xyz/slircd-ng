//! WEBIRC command handler for trusted web gateways.

use super::super::{Context, HandlerResult, PreRegHandler};
use crate::config::WebircBlock;
use crate::state::UnregisteredState;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, wildcard_match};
use tracing::{debug, info, warn};

/// Handler for WEBIRC command.
///
/// `WEBIRC password gateway hostname ip`
///
/// Allows trusted web gateways/proxies to forward real client information.
/// Must be sent before NICK/USER registration.
pub struct WebircHandler {
    /// Configured WEBIRC blocks from server config.
    pub webirc_blocks: Vec<WebircBlock>,
}

impl WebircHandler {
    /// Create a new WebircHandler with the given configuration.
    pub fn new(webirc_blocks: Vec<WebircBlock>) -> Self {
        Self { webirc_blocks }
    }

    /// Check if a WEBIRC request is authorized.
    fn is_authorized(&self, password: &str, gateway_host: &str) -> bool {
        for block in &self.webirc_blocks {
            if block.password == password {
                // If no hosts specified, accept from anywhere
                if block.hosts.is_empty() {
                    return true;
                }
                // Check if gateway_host matches any allowed pattern
                for host_pattern in &block.hosts {
                    if wildcard_match(host_pattern, gateway_host) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[async_trait]
impl PreRegHandler for WebircHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // WEBIRC must be sent before NICK/USER
        if ctx.state.nick.is_some() || ctx.state.user.is_some() {
            // Silently ignore WEBIRC after registration has started
            debug!("WEBIRC rejected: registration already started");
            return Ok(());
        }

        // WEBIRC <password> <gateway> <hostname> <ip>
        let password = match msg.arg(0) {
            Some(p) if !p.is_empty() => p,
            _ => {
                debug!("WEBIRC rejected: missing password");
                return Ok(());
            }
        };

        let gateway = match msg.arg(1) {
            Some(g) if !g.is_empty() => g,
            _ => {
                debug!("WEBIRC rejected: missing gateway");
                return Ok(());
            }
        };

        let hostname = match msg.arg(2) {
            Some(h) if !h.is_empty() => h,
            _ => {
                debug!("WEBIRC rejected: missing hostname");
                return Ok(());
            }
        };

        let ip = match msg.arg(3) {
            Some(i) if !i.is_empty() => i,
            _ => {
                debug!("WEBIRC rejected: missing IP");
                return Ok(());
            }
        };

        // Get the gateway's connecting IP for authorization check
        let gateway_ip = ctx.remote_addr.ip().to_string();

        // Check authorization
        if !self.is_authorized(password, &gateway_ip) {
            warn!(
                gateway = %gateway,
                gateway_ip = %gateway_ip,
                "WEBIRC rejected: invalid password or unauthorized host"
            );
            // Disconnect the client for security
            let error_msg = Message {
                tags: None,
                prefix: None,
                command: Command::ERROR("WEBIRC authentication failed".to_string()),
            };
            ctx.sender.send(error_msg).await?;
            return Ok(());
        }

        // Store WEBIRC info in handshake state
        ctx.state.webirc_used = true;
        ctx.state.webirc_ip = Some(ip.to_string());
        ctx.state.webirc_host = Some(hostname.to_string());

        info!(
            gateway = %gateway,
            real_ip = %ip,
            real_host = %hostname,
            gateway_ip = %gateway_ip,
            "WEBIRC accepted"
        );

        Ok(())
    }
}
