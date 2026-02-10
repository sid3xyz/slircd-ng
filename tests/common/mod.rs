//! Integration test common infrastructure.
//!
//! Provides utilities for spawning test servers, creating test clients,
//! and asserting on IRC message flows.

pub mod client;
pub mod server;
pub mod tls;

#[allow(unused_imports)]
pub use client::TestClient;
#[allow(unused_imports)]
pub use server::TestServer;
