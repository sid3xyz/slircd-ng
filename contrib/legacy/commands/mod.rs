//! Command Registry Pattern - Modern IRC command architecture
//!
//! This module implements a clean command pattern for IRC command handling:
//! 
//! 1. **Command Trait** - Common interface for all commands
//! 2. **Command Registry** - Central parser dispatcher
//! 3. **Execution Context** - State and capabilities for command execution
//! 4. **Command Implementations** - Each IRC command as independent module
//!
//! # Architecture
//!
//! ```text
//! Client Socket
//!      ↓
//! SessionActor.reader_task
//!      ↓
//! CommandRegistry::parse(line)
//!      ↓
//! Box<dyn Command>
//!      ↓
//! BrokerActor.handle_command()
//!      ↓
//! command.execute(ctx)
//! ```
//!
//! # Adding New Commands
//!
//! 1. Create `src/commands/core/yourcommand.rs`:
//!    ```rust
//!    use crate::commands::r#trait::{Command, RegistrationLevel};
//!    use crate::commands::context::ExecutionContext;
//!    
//!    #[derive(Debug, Clone)]
//!    pub struct YourCommand { /* fields */ }
//!    
//!    impl Command for YourCommand {
//!        fn parse(parts: &[&str]) -> Result<Box<dyn Command>> { /* ... */ }
//!        fn execute(&self, ctx: &mut ExecutionContext) -> Result<()> { /* ... */ }
//!        fn name(&self) -> &'static str { "YOURCOMMAND" }
//!    }
//!    ```
//!
//! 2. Register in `registry.rs`:
//!    ```rust
//!    reg.register("YOURCOMMAND", YourCommand::parse);
//!    ```
//!
//! 3. Add unit tests to your command module
//!
//! # Migration Status
//!
//! - [x] Infrastructure (trait, registry, context)
//! - [x] NICK command
//! - [x] MODE command
//! - [ ] USER command
//! - [ ] PRIVMSG/NOTICE commands
//! - [ ] JOIN/PART commands
//! - [ ] Remaining RFC 2812 commands

pub mod r#trait;
pub mod context;
pub mod registry;
pub mod core;

pub use r#trait::{Command, RegistrationLevel};
pub use context::ExecutionContext;
pub use registry::{CommandRegistry, REGISTRY};
