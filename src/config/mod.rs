//! Configuration loading and management.
//!
//! This module is split into logical submodules:
//! - [`types`]: Core config struct definitions (Config, ServerConfig, ListenConfig)
//! - [`listen`]: Network listener configuration (ListenConfig, TlsConfig, WebSocketConfig)
//! - [`security`]: Security configuration (SecurityConfig, SpamConfig, RateLimitConfig, HeuristicsConfig)
//! - [`history`]: History storage configuration (HistoryConfig, HistoryEventsConfig)
//! - [`limits`]: Output limits configuration (LimitsConfig)
//! - [`oper`]: Operator and WEBIRC block configuration (OperBlock, WebircBlock)
//! - [`links`]: Server-to-server link configuration (LinkBlock)

mod history;
mod limits;
mod links;
mod listen;
mod multiclient;
mod oper;
mod security;
mod types;

// Re-export all public types for convenient access
// Some may be unused currently but are part of the public API
pub use history::HistoryConfig;
pub use limits::LimitsConfig;
pub use links::LinkBlock;
pub use listen::{ClientAuth, ListenConfig, S2STlsConfig, StsConfig, TlsConfig, WebSocketConfig};
pub use multiclient::{AlwaysOnPolicy, MulticlientConfig};
pub use oper::{OperBlock, WebircBlock};
pub use security::{HeuristicsConfig, RateLimitConfig, RblConfig, SecurityConfig};
pub use types::{AccountRegistrationConfig, Casemapping, Config, IdleTimeoutsConfig, ServerConfig};
