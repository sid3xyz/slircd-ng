//! Prometheus Metrics Exporter Plugin
//!
//! HTTP endpoint that exposes core metrics in Prometheus text format.
//!
//! # Architecture (Big 4 Pattern)
//!
//! - **Core**: Always collects metrics (src/infrastructure/observability/)
//! - **This Plugin**: Optionally exports via HTTP /metrics endpoint
//!
//! # Usage
//!
//! Config:
//! ```toml
//! [plugins.prometheus]
//! enabled = true
//! bind_addr = "127.0.0.1:9090"
//! ```
//!
//! Access: `curl http://localhost:9090/metrics`

pub mod server;

use crate::plugin_api::{Plugin, PluginMetadata, MetricsExporterPlugin};
use crate::core::state::ServerState;
use async_trait::async_trait;
use std::sync::Arc;
use anyhow::Result;
use vise::{MetricsCollection, Registry};

/// Configuration for Prometheus HTTP exporter
#[derive(Debug, Clone)]
pub struct PrometheusConfig {
    /// Bind address for HTTP server (e.g., "127.0.0.1:9090")
    pub bind_addr: String,
    /// Enable admin panel at /admin (includes real-time user/channel list)
    pub enable_admin_panel: bool,
}

impl PrometheusConfig {
    /// Create config from environment variables
    /// - SLIRCD_METRICS_HOST (default: 127.0.0.1)
    /// - SLIRCD_METRICS_PORT (default: 9090)
    pub fn from_env() -> Self {
        let host = std::env::var("SLIRCD_METRICS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("SLIRCD_METRICS_PORT").unwrap_or_else(|_| "9090".to_string());
        Self {
            bind_addr: format!("{}:{}", host, port),
            enable_admin_panel: true,
        }
    }
}

impl Default for PrometheusConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:9090".to_string(),
            enable_admin_panel: true,
        }
    }
}

/// Prometheus HTTP exporter plugin
///
/// Serves Prometheus-compatible metrics at /metrics endpoint.
/// Reads from core METRICS registry (always collecting).
pub struct PrometheusPlugin {
    config: PrometheusConfig,
    state: Option<Arc<ServerState>>,
}

impl PrometheusPlugin {
    /// Create new Prometheus exporter with configuration
    pub fn new(config: PrometheusConfig) -> Self {
        Self {
            config,
            state: None,
        }
    }

    /// Set server state reference (for admin panel dynamic metrics)
    pub fn with_state(mut self, state: Arc<ServerState>) -> Self {
        self.state = Some(state);
        self
    }

    /// Get plugin configuration
    pub fn config(&self) -> &PrometheusConfig {
        &self.config
    }
}

#[async_trait]
impl Plugin for PrometheusPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "prometheus".to_string(),
            name: "Prometheus HTTP Exporter".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "SLIRCd Team".to_string(),
            description: "HTTP endpoint for Prometheus metrics scraping".to_string(),
            dependencies: vec![],
        }
    }

    async fn on_load(&mut self) -> Result<()> {
        tracing::info!(
            bind_addr = %self.config.bind_addr,
            admin_panel = self.config.enable_admin_panel,
            "Prometheus exporter plugin loading"
        );
        Ok(())
    }

    async fn on_enable(&mut self) -> Result<()> {
        tracing::info!("Prometheus exporter enabled - starting HTTP server");
        
        // Start HTTP server in background
        // Note: WebAdmin config is passed via main.rs startup, not via plugin system
        if let Some(ref state) = self.state {
            let config = self.config.clone();
            let state_clone = Arc::clone(state);
            
            tokio::spawn(async move {
                // Plugin system call passes None for webadmin (main.rs handles it)
                if let Err(e) = server::start_metrics_server(config, None, state_clone).await {
                    tracing::error!("Prometheus HTTP server error: {}", e);
                }
            });
        }
        
        Ok(())
    }

    async fn on_disable(&mut self) -> Result<()> {
        tracing::info!("Prometheus exporter disabled");
        // Note: HTTP server will stop when tokio task is cancelled
        Ok(())
    }

    async fn on_unload(&mut self) -> Result<()> {
        tracing::info!("Prometheus exporter plugin unloaded");
        Ok(())
    }

    async fn health_check(&self) -> Result<()> {
        // Plugin is healthy if it loaded successfully
        Ok(())
    }
}

#[async_trait]
impl MetricsExporterPlugin for PrometheusPlugin {
    async fn export_metrics(&self) -> Result<String> {
        // Export core METRICS in Prometheus text format
        let registry: Registry = MetricsCollection::default().collect();
        let mut text = String::new();
        registry.encode(&mut text, vise::Format::OpenMetrics)?;
        Ok(text)
    }

    fn bind_addr(&self) -> Option<String> {
        Some(self.config.bind_addr.clone())
    }
}
