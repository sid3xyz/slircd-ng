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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_block(password: &str, hosts: Vec<&str>) -> WebircBlock {
        WebircBlock {
            password: password.to_string(),
            hosts: hosts.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    // ========================================================================
    // is_authorized tests
    // ========================================================================

    #[test]
    fn is_authorized_correct_password_no_hosts() {
        let handler = WebircHandler::new(vec![make_block("secret123", vec![])]);
        // Empty hosts list = accept from anywhere
        assert!(handler.is_authorized("secret123", "192.168.1.1"));
        assert!(handler.is_authorized("secret123", "any.host.com"));
    }

    #[test]
    fn is_authorized_wrong_password() {
        let handler = WebircHandler::new(vec![make_block("secret123", vec![])]);
        assert!(!handler.is_authorized("wrongpass", "192.168.1.1"));
        assert!(!handler.is_authorized("", "192.168.1.1"));
    }

    #[test]
    fn is_authorized_correct_password_matching_host() {
        let handler = WebircHandler::new(vec![make_block("secret", vec!["192.168.1.*"])]);
        assert!(handler.is_authorized("secret", "192.168.1.100"));
        assert!(handler.is_authorized("secret", "192.168.1.1"));
    }

    #[test]
    fn is_authorized_correct_password_non_matching_host() {
        let handler = WebircHandler::new(vec![make_block("secret", vec!["192.168.1.*"])]);
        assert!(!handler.is_authorized("secret", "192.168.2.100"));
        assert!(!handler.is_authorized("secret", "10.0.0.1"));
    }

    #[test]
    fn is_authorized_multiple_blocks() {
        let handler = WebircHandler::new(vec![
            make_block("pass1", vec!["10.0.0.*"]),
            make_block("pass2", vec!["192.168.*"]),
        ]);
        assert!(handler.is_authorized("pass1", "10.0.0.5"));
        assert!(handler.is_authorized("pass2", "192.168.1.1"));
        assert!(!handler.is_authorized("pass1", "192.168.1.1"));
        assert!(!handler.is_authorized("pass2", "10.0.0.5"));
    }

    #[test]
    fn is_authorized_wildcard_host_patterns() {
        let handler = WebircHandler::new(vec![make_block(
            "web",
            vec!["*.example.com", "gateway.*.net"],
        )]);
        assert!(handler.is_authorized("web", "proxy.example.com"));
        assert!(handler.is_authorized("web", "gateway.test.net"));
        assert!(!handler.is_authorized("web", "evil.example.org"));
    }

    #[test]
    fn is_authorized_no_blocks() {
        let handler = WebircHandler::new(vec![]);
        assert!(!handler.is_authorized("anypass", "anyhost"));
    }

    #[test]
    fn is_authorized_exact_host_match() {
        let handler = WebircHandler::new(vec![make_block("exact", vec!["trusted.gateway.com"])]);
        assert!(handler.is_authorized("exact", "trusted.gateway.com"));
        assert!(!handler.is_authorized("exact", "other.gateway.com"));
    }
}
