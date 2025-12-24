//! Network module.
//!
//! Contains the Gateway (TCP listener) and Connection handler.

mod connection;
mod gateway;
mod proxy_protocol;

pub use connection::Connection;
pub use gateway::Gateway;
