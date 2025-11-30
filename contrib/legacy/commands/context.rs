//! Execution context - state and capabilities available to commands
//!
//! Provides commands with:
//! - Client identity and state
//! - Server state access (clients, channels)
//! - Response channel for sending IRC replies
//! - Metrics and logging context

use crate::actors::state::{ClientId, ClientState, ServerState};
use crate::actors::messages::SessionMessage;
use tokio::sync::mpsc;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Execution context provided to command.execute()
///
/// Contains everything a command needs to:
/// - Read client/server state
/// - Modify state (via ServerState methods)
/// - Send responses to client
/// - Log/metric operations
#[derive(Debug)]
pub struct ExecutionContext {
    /// Client executing this command
    pub client_id: ClientId,
    
    /// Client state (nickname, registration, modes, etc.)
    pub client_state: ClientState,
    
    /// Server state (clients, channels, configuration)
    pub server_state: Arc<RwLock<ServerState>>,
    
    /// Channel to send responses to client's SessionActor
    pub response_tx: mpsc::UnboundedSender<SessionMessage>,
}

impl ExecutionContext {
    /// Create execution context for a command
    pub fn new(
        client_id: ClientId,
        client_state: ClientState,
        server_state: Arc<RwLock<ServerState>>,
        response_tx: mpsc::UnboundedSender<SessionMessage>,
    ) -> Self {
        Self {
            client_id,
            client_state,
            server_state,
            response_tx,
        }
    }

    /// Send raw IRC message to client
    pub fn send_raw(&self, data: impl AsRef<str>) -> anyhow::Result<()> {
        let bytes = format!("{}\r\n", data.as_ref()).into_bytes();
        self.response_tx.send(SessionMessage::Write { data: bytes })
            .map_err(|e| anyhow::anyhow!("Failed to send response: {}", e))
    }

    /// Send numeric reply to client
    /// 
    /// # Example
    /// ```ignore
    /// ctx.send_numeric(221, &["+i"], "User mode +i set")?;
    /// // -> :server 221 nick +i :User mode +i set
    /// ```
    pub fn send_numeric(&self, code: u16, params: &[&str], trailing: &str) -> anyhow::Result<()> {
        let nick = self.client_state.nickname.as_deref().unwrap_or("*");
        let param_str = if params.is_empty() {
            String::new()
        } else {
            format!(" {}", params.join(" "))
        };
        
        let msg = format!(":server {:03} {}{} :{}", code, nick, param_str, trailing);
        self.send_raw(msg)
    }

    /// Disconnect client with reason
    pub fn disconnect(&self, reason: impl AsRef<str>) -> anyhow::Result<()> {
        self.response_tx.send(SessionMessage::Disconnect { 
            reason: reason.as_ref().to_string() 
        })
            .map_err(|e| anyhow::anyhow!("Failed to disconnect: {}", e))
    }
}
