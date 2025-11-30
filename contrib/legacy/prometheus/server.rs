//! HTTP metrics endpoint for Prometheus scraping.
//!
//! Reads from core METRICS registry (src/infrastructure/observability/).
//! Provides HTTP endpoints:
//! - /metrics: Prometheus text format (for scraping)
//! - /health: Health check (always 200 OK)
//! - /admin: Browser dashboard (real-time stats) or WebAdmin UI
//! - /api/admin/*: WebAdmin REST API (if enabled)

use crate::Result;
use crate::core::state::ServerState;
use crate::extensions::webadmin::{auth::Authenticator, events::EventLog, handlers::RequestHandler};
use crate::infrastructure::config::config::WebAdminSection;

use super::PrometheusConfig;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{info, warn};
use vise::{MetricsCollection, Registry};

/// Start the Prometheus HTTP server (optionally with WebAdmin)
pub async fn start_metrics_server(
    config: PrometheusConfig,
    webadmin_config: Option<WebAdminSection>,
    state: Arc<ServerState>,
) -> Result<()> {
    let bind_addr: SocketAddr = config.bind_addr.parse()?;
    let listener = TcpListener::bind(bind_addr).await?;
    info!(addr = %bind_addr, "Prometheus metrics server listening");

    // Initialize WebAdmin components if enabled
    let webadmin_handler: Option<Arc<RequestHandler>> = if let Some(ref wconfig) = webadmin_config {
        if wconfig.enabled {
            if wconfig.password_hash.is_none() {
                warn!("WebAdmin enabled but no password_hash configured - API will return 401");
            }
            
            let authenticator = Arc::new(Authenticator::new(
                wconfig.username.clone(),
                wconfig.password_hash.clone().unwrap_or_default(),
                wconfig.max_actions_per_minute as u32,
            ));
            
            let event_log = Arc::new(EventLog::new(wconfig.max_log_entries));
            
            info!("WebAdmin enabled - REST API available at /api/admin/*");
            
            Some(Arc::new(RequestHandler::new(
                authenticator,
                Arc::clone(&state),
                event_log,
            )))
        } else {
            None
        }
    } else {
        None
    };

    // Collect all registered metrics into a registry
    let _registry: Registry = MetricsCollection::default().collect();

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let state_clone = Arc::clone(&state);
                let webadmin_clone = webadmin_handler.clone();
                let webadmin_enabled = webadmin_config.as_ref().map(|c| c.enabled).unwrap_or(false);
                let remote_addr = addr.to_string(); // Capture socket address for audit logging
                
                // Move the registry into the task by recreating it each time
                tokio::spawn(async move {
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(
                            hyper_util::rt::TokioIo::new(stream),
                            service_fn(move |req| {
                                let registry: Registry = MetricsCollection::default().collect();
                                let state = Arc::clone(&state_clone);
                                let webadmin = webadmin_clone.clone();
                                let addr_str = remote_addr.clone();
                                handle_request(req, registry, state, webadmin, webadmin_enabled, addr_str)
                            }),
                        )
                        .await
                    {
                        warn!(addr = %addr, error = ?err, "metrics server connection error");
                    }
                });
            }
            Err(err) => {
                warn!(error = ?err, "failed to accept metrics connection");
            }
        }
    }
}

/// Handle incoming HTTP requests for metrics
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    registry: Registry,
    state: Arc<ServerState>,
    webadmin_handler: Option<Arc<RequestHandler>>,
    webadmin_enabled: bool,
    remote_addr: String,
) -> std::result::Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    
    // Extract real IP considering reverse proxy headers
    let real_ip = crate::extensions::webadmin::auth::extract_real_ip(req.headers(), &remote_addr);
    
    // Route WebAdmin API requests if enabled
    if webadmin_enabled && path.starts_with("/api/admin/") {
        if let Some(handler) = webadmin_handler {
            match handler.handle(req, real_ip).await {
                Ok(response) => return Ok(response),
                Err(_) => {
                    // Error already logged in handler
                    let mut builder = Response::builder();
                    builder = builder.status(StatusCode::INTERNAL_SERVER_ERROR);
                    builder = builder.header("content-type", "application/json");
                    let body = Full::new(Bytes::from(r#"{"success":false,"message":"Internal server error"}"#));
                    return Ok(builder.body(body).unwrap_or_else(|_| Response::new(Full::new(Bytes::from("")))));
                }
            }
        }
    }
    
    match (&method, path.as_str()) {
        (&Method::GET, "/") | (&Method::GET, "/admin") => {
            // Require authentication for WebAdmin UI (if enabled)
            if webadmin_enabled {
                // Check if request is authenticated before serving WebAdmin UI
                if let Some(handler) = webadmin_handler.clone() {
                    match handler.authenticate_request(&req, &real_ip).await {
                        Ok((_username, _creds)) => {
                            // Authentication successful - serve WebAdmin UI
                            let html = crate::extensions::webadmin::ui::get_admin_panel_html();
                            
                            let mut builder = Response::builder();
                            builder = builder.status(StatusCode::OK);
                            builder = builder.header("content-type", "text/html; charset=utf-8");
                            let body = Full::new(Bytes::from(html));
                            return Ok(builder
                                .body(body)
                                .unwrap_or_else(|_| Response::new(Full::new(Bytes::from("")))));
                        }
                        Err(_) => {
                            // Authentication failed - return 401 with Basic auth challenge
                            let mut builder = Response::builder();
                            builder = builder.status(StatusCode::UNAUTHORIZED);
                            builder = builder.header("content-type", "text/html; charset=utf-8");
                            builder = builder.header("www-authenticate", "Basic realm=\"SLIRCd WebAdmin\"");
                            let body = Full::new(Bytes::from(
                                r#"<!DOCTYPE html><html><head><title>SLIRCd WebAdmin - Authentication Required</title></head>
<body style="font-family: sans-serif; margin: 50px; text-align: center;">
<h1>🔒 Authentication Required</h1>
<p>Please provide your WebAdmin credentials to access the admin panel.</p>
<p style="color: #666; margin-top: 30px;">Contact your IRC server administrator if you don't have credentials.</p>
</body></html>"#));
                            return Ok(builder
                                .body(body)
                                .unwrap_or_else(|_| Response::new(Full::new(Bytes::from("")))));
                        }
                    }
                }
            }
            
            // Fall back to basic admin panel if WebAdmin is disabled
            let html = get_admin_panel_html();
            let mut builder = Response::builder();
            builder = builder.status(StatusCode::OK);
            builder = builder.header("content-type", "text/html; charset=utf-8");
            let body = Full::new(Bytes::from(html));
            Ok(builder
                .body(body)
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::from("")))))
        }
        (&Method::GET, "/api/stats") => {
            // JSON API for server stats
            let stats = get_server_stats(state).await;
            let mut builder = Response::builder();
            builder = builder.status(StatusCode::OK);
            builder = builder.header("content-type", "application/json");
            builder = builder.header("access-control-allow-origin", "*");
            let body = Full::new(Bytes::from(stats));
            Ok(builder
                .body(body)
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::from("{}")))))
        }
        (&Method::GET, "/metrics") => {
            // Export metrics in OpenMetrics format
            let mut metrics_text = String::new();
            if let Err(e) = registry.encode(&mut metrics_text, vise::Format::OpenMetrics) {
                tracing::warn!(error = ?e, "failed to encode metrics");
                let mut builder = Response::builder();
                builder = builder.status(StatusCode::INTERNAL_SERVER_ERROR);
                builder = builder.header("content-type", "text/plain");
                let body = Full::new(Bytes::from("Internal Server Error"));
                return Ok(builder.body(body).unwrap_or_else(|_| {
                    Response::new(Full::new(Bytes::from("Internal Server Error")))
                }));
            }

            let mut builder = Response::builder();
            builder = builder.status(StatusCode::OK);
            builder = builder.header(
                "content-type",
                "application/openmetrics-text; version=1.0.0; charset=utf-8",
            );
            let body = Full::new(Bytes::from(metrics_text));
            Ok(builder
                .body(body)
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::from("")))))
        }
        (&Method::GET, "/health") => {
            // Simple health check endpoint
            let mut builder = Response::builder();
            builder = builder.status(StatusCode::OK);
            builder = builder.header("content-type", "text/plain");
            Ok(builder
                .body(Full::new(Bytes::from("OK")))
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::from("OK")))))
        }
        _ => {
            // Return 404 for all other requests
            let mut builder = Response::builder();
            builder = builder.status(StatusCode::NOT_FOUND);
            builder = builder.header("content-type", "text/plain");
            Ok(builder
                .body(Full::new(Bytes::from("Not Found")))
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::from("Not Found")))))
        }
    }
}

/// Generate server stats JSON
async fn get_server_stats(state: Arc<ServerState>) -> String {
    let total_clients = state.get_client_count();
    let total_channels = state.list_channels(0).await.len(); // Use client_id 0 for admin view
    
    // Get list of connected users
    let mut users = Vec::new();
    let max_check = (total_clients * 2).min(1000); // Cap at 1000 to avoid slow scans
    for client_id in 1..=max_check as u64 {
        if let Some(client) = state.get_client(client_id).await {
            if let Some(nick) = &client.nickname {
                users.push(serde_json::json!({
                    "id": client_id,
                    "nick": nick,
                    "username": client.username.as_deref().unwrap_or("*"),
                    "hostname": &client.display_hostname,
                    "registered": client.is_registered(),
                }));
            }
        }
    }
    
    // Get list of channels
    let mut channels = Vec::new();
    for (name, member_count, _topic) in state.list_channels(0).await {
        channels.push(serde_json::json!({
            "name": name,
            "members": member_count,
        }));
    }
    
    let stats = serde_json::json!({
        "server": {
            "name": state.server_name(),
            "uptime_seconds": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        },
        "stats": {
            "total_clients": total_clients,
            "total_channels": total_channels,
        },
        "users": users,
        "channels": channels,
    });
    
    serde_json::to_string_pretty(&stats).unwrap_or_else(|_| "{}".to_string())
}

/// Generate admin panel HTML
fn get_admin_panel_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>SLIRCd Admin Panel</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: #333;
            padding: 20px;
        }
        .container {
            max-width: 1400px;
            margin: 0 auto;
        }
        header {
            background: rgba(255,255,255,0.95);
            padding: 20px 30px;
            border-radius: 15px;
            box-shadow: 0 10px 40px rgba(0,0,0,0.2);
            margin-bottom: 30px;
        }
        h1 {
            color: #667eea;
            font-size: 2.5em;
            margin-bottom: 5px;
        }
        .subtitle {
            color: #666;
            font-size: 0.9em;
        }
        .grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }
        .card {
            background: rgba(255,255,255,0.95);
            padding: 25px;
            border-radius: 15px;
            box-shadow: 0 10px 40px rgba(0,0,0,0.15);
        }
        .card h2 {
            color: #667eea;
            font-size: 1.3em;
            margin-bottom: 15px;
            border-bottom: 2px solid #667eea;
            padding-bottom: 10px;
        }
        .stat {
            display: flex;
            justify-content: space-between;
            padding: 12px 0;
            border-bottom: 1px solid #eee;
        }
        .stat:last-child { border-bottom: none; }
        .stat-label {
            color: #666;
            font-weight: 500;
        }
        .stat-value {
            color: #333;
            font-weight: 700;
            font-size: 1.1em;
        }
        .user-list, .channel-list {
            max-height: 400px;
            overflow-y: auto;
        }
        .user-item, .channel-item {
            padding: 12px;
            margin: 8px 0;
            background: #f8f9fa;
            border-radius: 8px;
            border-left: 4px solid #667eea;
        }
        .user-item:hover, .channel-item:hover {
            background: #e9ecef;
            transform: translateX(5px);
            transition: all 0.2s;
        }
        .user-nick {
            font-weight: 700;
            color: #667eea;
            font-size: 1.1em;
        }
        .user-host {
            color: #666;
            font-size: 0.9em;
            margin-top: 5px;
        }
        .channel-name {
            font-weight: 700;
            color: #764ba2;
            font-size: 1.1em;
        }
        .channel-count {
            color: #666;
            font-size: 0.9em;
        }
        .loading {
            text-align: center;
            padding: 40px;
            color: #666;
            font-style: italic;
        }
        .refresh-btn {
            background: #667eea;
            color: white;
            border: none;
            padding: 12px 30px;
            border-radius: 8px;
            cursor: pointer;
            font-size: 1em;
            font-weight: 600;
            box-shadow: 0 4px 15px rgba(102,126,234,0.4);
            transition: all 0.3s;
        }
        .refresh-btn:hover {
            background: #5568d3;
            transform: translateY(-2px);
            box-shadow: 0 6px 20px rgba(102,126,234,0.5);
        }
        .auto-refresh {
            display: flex;
            align-items: center;
            gap: 10px;
            margin-top: 15px;
        }
        .auto-refresh input[type="checkbox"] {
            width: 20px;
            height: 20px;
            cursor: pointer;
        }
        @keyframes pulse {
            0%, 100% { opacity: 1; }
            50% { opacity: 0.5; }
        }
        .updating {
            animation: pulse 1s infinite;
        }
    </style>
</head>
<body>
    <div class="container">
        <header>
            <h1>🚀 SLIRCd Admin Panel</h1>
            <p class="subtitle">Real-time IRC Server Monitoring & Management</p>
            <div style="margin-top: 15px;">
                <button class="refresh-btn" onclick="loadStats()">🔄 Refresh Now</button>
                <div class="auto-refresh">
                    <input type="checkbox" id="autoRefresh" checked onchange="toggleAutoRefresh()">
                    <label for="autoRefresh">Auto-refresh every 5 seconds</label>
                </div>
            </div>
        </header>

        <div class="grid">
            <div class="card">
                <h2>📊 Server Statistics</h2>
                <div id="stats" class="loading">Loading...</div>
            </div>
            <div class="card">
                <h2>👥 Connected Users (<span id="userCount">0</span>)</h2>
                <div id="users" class="user-list loading">Loading...</div>
            </div>
            <div class="card">
                <h2>📢 Active Channels (<span id="channelCount">0</span>)</h2>
                <div id="channels" class="channel-list loading">Loading...</div>
            </div>
        </div>
    </div>

    <script>
        let autoRefreshInterval = null;

        async function loadStats() {
            try {
                document.getElementById('stats').classList.add('updating');
                const response = await fetch('/api/stats');
                const data = await response.json();

                // Update server stats
                const statsHtml = `
                    <div class="stat">
                        <span class="stat-label">Server Name</span>
                        <span class="stat-value">${data.server.name}</span>
                    </div>
                    <div class="stat">
                        <span class="stat-label">Total Clients</span>
                        <span class="stat-value">${data.stats.total_clients}</span>
                    </div>
                    <div class="stat">
                        <span class="stat-label">Total Channels</span>
                        <span class="stat-value">${data.stats.total_channels}</span>
                    </div>
                    <div class="stat">
                        <span class="stat-label">Last Update</span>
                        <span class="stat-value">${new Date().toLocaleTimeString()}</span>
                    </div>
                `;
                document.getElementById('stats').innerHTML = statsHtml;
                document.getElementById('stats').classList.remove('updating');

                // Update users
                document.getElementById('userCount').textContent = data.users.length;
                const usersHtml = data.users.length === 0 
                    ? '<div class="loading">No users connected</div>'
                    : data.users.map(user => `
                        <div class="user-item">
                            <div class="user-nick">${user.nick}</div>
                            <div class="user-host">${user.username}@${user.hostname}</div>
                        </div>
                    `).join('');
                document.getElementById('users').innerHTML = usersHtml;

                // Update channels
                document.getElementById('channelCount').textContent = data.channels.length;
                const channelsHtml = data.channels.length === 0
                    ? '<div class="loading">No channels created</div>'
                    : data.channels.map(channel => `
                        <div class="channel-item">
                            <div class="channel-name">${channel.name}</div>
                            <div class="channel-count">${channel.members} member${channel.members !== 1 ? 's' : ''}</div>
                        </div>
                    `).join('');
                document.getElementById('channels').innerHTML = channelsHtml;

            } catch (error) {
                console.error('Failed to load stats:', error);
                document.getElementById('stats').innerHTML = '<div class="loading">❌ Failed to load stats</div>';
            }
        }

        function toggleAutoRefresh() {
            const enabled = document.getElementById('autoRefresh').checked;
            if (enabled) {
                autoRefreshInterval = setInterval(loadStats, 5000);
            } else {
                if (autoRefreshInterval) {
                    clearInterval(autoRefreshInterval);
                    autoRefreshInterval = null;
                }
            }
        }

        // Initial load
        loadStats();
        toggleAutoRefresh();
    </script>
</body>
</html>"#.to_string()
}
