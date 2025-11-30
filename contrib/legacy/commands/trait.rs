//! Command trait - core abstraction for IRC command execution
//!
//! Each IRC command implements this trait, providing:
//! - Parsing from raw IRC line tokens
//! - Execution within an execution context
//! - Metadata (name, min registration level)

use anyhow::Result;
use super::context::ExecutionContext;

/// IRC command that can be parsed and executed
///
/// Implementation pattern:
/// 1. Parse tokens into command struct
/// 2. Execute with access to execution context
/// 3. Context provides state, capabilities, response channel
pub trait Command: Send + Sync + std::fmt::Debug {
    /// Parse command from IRC line tokens
    /// 
    /// # Arguments
    /// * `parts` - Tokenized IRC line (command already uppercased, at index 0)
    /// 
    /// # Example
    /// ```ignore
    /// // "NICK alice" -> parts = ["NICK", "alice"]
    /// // "MODE bob +i" -> parts = ["MODE", "bob", "+i"]
    /// ```
    fn parse(parts: &[&str]) -> Result<Box<dyn Command>>
    where
        Self: Sized;

    /// Execute command with execution context
    /// 
    /// Context provides:
    /// - Client state (registered, nickname, etc.)
    /// - Server state (channels, clients, etc.)
    /// - Response channel to send replies
    fn execute(&self, ctx: &mut ExecutionContext) -> Result<()>;

    /// Command name (e.g., "NICK", "MODE", "PRIVMSG")
    fn name(&self) -> &'static str;

    /// Minimum registration level required to execute
    /// 
    /// - None: Pre-registration (PASS, NICK, USER, CAP)
    /// - Partial: NICK+USER complete
    /// - Full: Fully registered
    fn min_registration(&self) -> RegistrationLevel {
        RegistrationLevel::Full
    }
}

/// Client registration state required to execute command
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistrationLevel {
    /// Pre-registration (PASS, CAP negotiation)
    None,
    
    /// Partial - NICK or USER received
    Partial,
    
    /// Full registration complete
    Full,
}
