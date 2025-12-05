//! HTTP server for Prometheus metrics endpoint.
//!
//! Runs on a separate tokio task and serves `/metrics` for Prometheus scraping.

use axum::{Router, routing::get};
use std::net::SocketAddr;

/// Handler for GET /metrics - returns Prometheus metrics in text format.
async fn metrics_handler() -> String {
    crate::metrics::gather_metrics()
}

/// Run the HTTP server for Prometheus metrics.
///
/// Binds to `0.0.0.0:port` and serves the `/metrics` endpoint.
/// This is a long-running task that should be spawned in the background.
pub async fn run_http_server(port: u16) {
    let app = Router::new().route("/metrics", get(metrics_handler));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Prometheus HTTP server listening on {}", addr);

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("Failed to bind HTTP server on {}: {}", addr, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("HTTP server error: {}", e);
    }
}
