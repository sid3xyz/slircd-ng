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
mod oper;
mod security;
mod types;

// Re-export all public types for convenient access
// Some may be unused currently but are part of the public API
#[allow(unused_imports)]
pub use history::{HistoryConfig, HistoryEventsConfig};
pub use limits::LimitsConfig;
pub use links::LinkBlock;
#[allow(unused_imports)]
pub use listen::{ClientAuth, ListenConfig, S2STlsConfig, StsConfig, TlsConfig, WebSocketConfig};
pub use oper::{OperBlock, WebircBlock};
#[allow(unused_imports)]
pub use security::{HeuristicsConfig, RateLimitConfig, RblConfig, SecurityConfig, SpamConfig};
#[allow(unused_imports)]
pub use types::{
    AccountRegistrationConfig, Config, ConfigError, DatabaseConfig, IdleTimeoutsConfig, MotdConfig,
    ServerConfig,
};
