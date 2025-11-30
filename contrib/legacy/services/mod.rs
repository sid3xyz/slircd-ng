//! IRC Services Integration - Embedded NickServ/ChanServ Implementation
//!
//! Architecture: Kairos Design (The Perfect Moment) - building services that are embedded (Ergo-style), extensible (protocol-ready), integrated (SASL accounts ↔ Services nicknames).
//!
//! Competitive Positioning: Ergo simplicity + Anope command syntax + Atheme permissions = unique hybrid.
//!
//! Phase 1: nickserv.rs (HELP/REGISTER), pseudo_client.rs, routing.rs.
//! Future: IDENTIFY, ChanServ REGISTER/OP, protocol compatibility.
//!
//! Key Decisions: Pseudo-clients in ClientManager (Ergo), IRCv3 account integration, command dispatch pattern.
//!
//! Reference: docs/SERVICES_ARCHITECTURE.md, reference-docs/competitors/analysis/services-integration.md

pub mod chanserv;
pub mod nickserv;
pub mod plugin;
pub mod pseudo_client;
pub mod routing;

// Re-export key types for convenience
pub use plugin::ServicesPlugin;
pub use pseudo_client::{spawn_services, ServiceBots};
pub use routing::is_service_target;
