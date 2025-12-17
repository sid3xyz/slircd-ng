//! Gateway - TCP/TLS listener that accepts incoming connections.
//!
//! The Gateway binds to sockets and spawns Connection tasks for each
//! incoming client. Supports both plaintext and TLS connections.

use crate::config::WebircBlock;
use crate::config::{TlsConfig, WebSocketConfig};
use crate::db::Database;
use crate::handlers::Registry;
use crate::network::Connection;
use crate::state::Matrix;
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::io::{BufReader, Cursor};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::version::{TLS12, TLS13};
use tokio_tungstenite::accept_hdr_async;
use tracing::{error, info, instrument, warn};

/// Validate an incoming connection against IP deny list and rate limits.
///
/// Returns `Some(uid)` if the connection should proceed, `None` if rejected.
/// This centralizes the common accept logic for all listener types (TLS, WebSocket, plaintext).
fn validate_connection(addr: &SocketAddr, matrix: &Matrix, listener_type: &str) -> Option<String> {
    // HOT PATH: Nanosecond-scale IP denial check (Roaring Bitmap)
    // This runs BEFORE any other checks for maximum efficiency
    if let Ok(deny_list) = matrix.ip_deny_list.read()
        && let Some(reason) = deny_list.check_ip(&addr.ip())
    {
        info!(%addr, %reason, "{} connection rejected by IP deny list", listener_type);
        return None;
    }

    // Check connection rate limit before accepting
    if !matrix.rate_limiter.check_connection_rate(addr.ip()) {
        warn!(%addr, "{} connection rate limit exceeded - rejecting", listener_type);
        return None;
    }

    info!(%addr, "{} connection accepted", listener_type);
    Some(matrix.uid_gen.next())
}

/// Check DNSBL and return false if connection should be rejected.
async fn check_dnsbl(matrix: &Matrix, ip: IpAddr, addr: SocketAddr) -> bool {
    if let Some(ref spam) = matrix.spam_detector
        && spam.check_ip_dnsbl(ip).await
    {
        warn!(%addr, "Connection rejected by DNSBL");
        return false;
    }
    true
}

/// Handle TLS connection after acceptance.
async fn handle_tls_connection(
    uid: String,
    stream: TcpStream,
    addr: SocketAddr,
    acceptor: TlsAcceptor,
    matrix: Arc<Matrix>,
    registry: Arc<Registry>,
    db: Database,
) {
    let ip = addr.ip();

    if !check_dnsbl(&matrix, ip, addr).await {
        matrix.rate_limiter.on_connection_end(ip);
        return;
    }

    match acceptor.accept(stream).await {
        Ok(tls_stream) => {
            let connection =
                Connection::new_tls(uid.clone(), tls_stream, addr, matrix.clone(), registry, db);
            if let Err(e) = connection.run().await {
                error!(%uid, %addr, error = %e, "TLS connection error");
            }
            matrix.rate_limiter.on_connection_end(ip);
            info!(%uid, %addr, "TLS connection closed");
        }
        Err(e) => {
            warn!(%addr, error = %e, "TLS handshake failed");
            matrix.rate_limiter.on_connection_end(ip);
        }
    }
}

/// Handle WebSocket connection after acceptance.
async fn handle_websocket_connection(
    uid: String,
    stream: TcpStream,
    addr: SocketAddr,
    allowed: Vec<String>,
    matrix: Arc<Matrix>,
    registry: Arc<Registry>,
    db: Database,
) {
    let ip = addr.ip();

    if !check_dnsbl(&matrix, ip, addr).await {
        matrix.rate_limiter.on_connection_end(ip);
        return;
    }

    // CORS validation callback for WebSocket handshake
    let cors_callback = |req: &http::Request<()>, response: http::Response<()>| {
        validate_websocket_cors(req, response, &allowed, addr)
    };

    match accept_hdr_async(stream, cors_callback).await {
        Ok(ws_stream) => {
            info!(%addr, "WebSocket handshake successful");
            let connection = Connection::new_websocket(
                uid.clone(),
                ws_stream,
                addr,
                matrix.clone(),
                registry,
                db,
            );
            if let Err(e) = connection.run().await {
                error!(%uid, %addr, error = %e, "WebSocket connection error");
            }
            matrix.rate_limiter.on_connection_end(ip);
            info!(%uid, %addr, "WebSocket connection closed");
        }
        Err(e) => {
            warn!(%addr, error = %e, "WebSocket handshake failed");
            matrix.rate_limiter.on_connection_end(ip);
        }
    }
}

/// Validate WebSocket CORS origin.
#[allow(clippy::result_large_err)]
fn validate_websocket_cors(
    req: &http::Request<()>,
    response: http::Response<()>,
    allowed: &[String],
    addr: SocketAddr,
) -> Result<http::Response<()>, http::Response<Option<String>>> {
    // If allow_origins is empty, DENY by default (secure)
    if allowed.is_empty() {
        warn!("WebSocket CORS: No origins configured, rejecting all");
        let response = http::Response::builder()
            .status(http::StatusCode::FORBIDDEN)
            .body(Some("No WebSocket origins configured".to_string()))
            .unwrap_or_else(|_| http::Response::new(Some("Internal Server Error".to_string())));
        return Err(response);
    }

    // Check for wildcard "*" - allows all origins
    if allowed.iter().any(|a| a == "*") {
        return Ok(response);
    }

    // Check Origin header against allowed origins
    if let Some(origin) = req.headers().get("Origin").and_then(|o| o.to_str().ok()) {
        if allowed.iter().any(|a| a == origin) {
            return Ok(response);
        }
        warn!(%addr, origin = %origin, "WebSocket CORS rejected");
    }

    // Reject with 403 Forbidden
    let response = http::Response::builder()
        .status(http::StatusCode::FORBIDDEN)
        .body(Some("CORS origin not allowed".to_string()))
        .unwrap_or_else(|_| http::Response::new(Some("Internal Server Error".to_string())));
    Err(response)
}

/// Handle plaintext connection after acceptance.
async fn handle_plaintext_connection(
    uid: String,
    stream: TcpStream,
    addr: SocketAddr,
    matrix: Arc<Matrix>,
    registry: Arc<Registry>,
    db: Database,
) {
    let ip = addr.ip();

    if !check_dnsbl(&matrix, ip, addr).await {
        matrix.rate_limiter.on_connection_end(ip);
        return;
    }

    let connection =
        Connection::new_plaintext(uid.clone(), stream, addr, matrix.clone(), registry, db);
    if let Err(e) = connection.run().await {
        error!(%uid, %addr, error = %e, "Plaintext connection error");
    }
    matrix.rate_limiter.on_connection_end(ip);
    info!(%uid, %addr, "Plaintext connection closed");
}

/// The Gateway accepts incoming TCP/TLS connections and spawns handlers.
pub struct Gateway {
    plaintext_listener: TcpListener,
    tls_listener: Option<(TcpListener, TlsAcceptor)>,
    websocket_listener: Option<(TcpListener, WebSocketConfig)>,
    matrix: Arc<Matrix>,
    registry: Arc<Registry>,
    db: Database,
}

impl Gateway {
    /// Bind the gateway to the specified addresses.
    pub async fn bind(
        addr: SocketAddr,
        tls_config: Option<TlsConfig>,
        websocket_config: Option<WebSocketConfig>,
        webirc_blocks: Vec<WebircBlock>,
        matrix: Arc<Matrix>,
        db: Database,
    ) -> anyhow::Result<Self> {
        let plaintext_listener = TcpListener::bind(addr).await?;
        let registry = Arc::new(Registry::new(webirc_blocks));
        info!(%addr, "Plaintext listener bound");

        let tls_listener = if let Some(tls_cfg) = tls_config {
            let tls_acceptor = Self::load_tls(&tls_cfg)?;
            let listener = TcpListener::bind(tls_cfg.address).await?;
            info!(address = %tls_cfg.address, "TLS listener bound");
            Some((listener, tls_acceptor))
        } else {
            None
        };

        let websocket_listener = if let Some(ws_cfg) = websocket_config {
            let listener = TcpListener::bind(ws_cfg.address).await?;
            info!(address = %ws_cfg.address, "WebSocket listener bound");
            Some((listener, ws_cfg))
        } else {
            None
        };

        Ok(Self {
            plaintext_listener,
            tls_listener,
            websocket_listener,
            matrix,
            registry,
            db,
        })
    }

    /// Load TLS certificates and create TlsAcceptor.
    fn load_tls(config: &TlsConfig) -> anyhow::Result<TlsAcceptor> {
        // Load certificates
        let cert_file = std::fs::read(&config.cert_path)?;
        let cert_reader = &mut BufReader::new(Cursor::new(cert_file));
        let certs: Vec<CertificateDer> = certs(cert_reader).collect::<Result<Vec<_>, _>>()?;

        if certs.is_empty() {
            anyhow::bail!("No certificates found in {}", config.cert_path);
        }

        // Load private key
        let key_file = std::fs::read(&config.key_path)?;
        let key_reader = &mut BufReader::new(Cursor::new(key_file));
        let mut keys: Vec<PrivateKeyDer> = pkcs8_private_keys(key_reader)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(PrivateKeyDer::from)
            .collect();

        if keys.is_empty() {
            anyhow::bail!("No private keys found in {}", config.key_path);
        }

        let key = keys.remove(0);

        // Build TLS server config with explicit minimum version enforcement.
        // Only TLS 1.2 and 1.3 are allowed - TLS 1.0/1.1 are rejected.
        // This prevents downgrade attacks and ensures modern cryptography.
        //
        // NOTE: rustls does not currently support OCSP stapling or CRL checking
        // for client certificates. SASL EXTERNAL clients using client certs
        // are trusted based on the certificate chain only. For high-security
        // deployments, consider additional out-of-band verification.
        let tls_config = ServerConfig::builder_with_protocol_versions(&[&TLS13, &TLS12])
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        info!("TLS configured with minimum version TLS 1.2");
        Ok(TlsAcceptor::from(Arc::new(tls_config)))
    }

    /// Run the gateway, accepting connections forever.
    /// Returns when a shutdown signal is received.
    #[instrument(skip(self), name = "gateway")]
    pub async fn run(self) -> anyhow::Result<()> {
        let matrix = Arc::clone(&self.matrix);
        let registry = Arc::clone(&self.registry);
        let db = self.db.clone();

        // Subscribe to shutdown signal
        let mut shutdown_rx = matrix.shutdown_tx.subscribe();

        // If TLS is configured, spawn a separate task for the TLS listener
        if let Some((tls_listener, tls_acceptor)) = self.tls_listener {
            let matrix_tls = Arc::clone(&matrix);
            let registry_tls = Arc::clone(&registry);
            let db_tls = db.clone();
            let mut shutdown_rx_tls = matrix.shutdown_tx.subscribe();

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        result = tls_listener.accept() => {
                            let Ok((stream, addr)) = result.inspect_err(|e| {
                                error!(error = %e, "Failed to accept TLS connection");
                            }) else { continue };

                            let Some(uid) = validate_connection(&addr, &matrix_tls, "TLS") else {
                                continue;
                            };

                            if !matrix_tls.rate_limiter.on_connection_start(addr.ip()) {
                                warn!(%addr, "Connection rejected: max connections per IP exceeded");
                                continue;
                            }

                            tokio::spawn(handle_tls_connection(
                                uid,
                                stream,
                                addr,
                                tls_acceptor.clone(),
                                Arc::clone(&matrix_tls),
                                Arc::clone(&registry_tls),
                                db_tls.clone(),
                            ));
                        }
                        _ = shutdown_rx_tls.recv() => {
                            info!("Shutdown signal received - stopping TLS listener");
                            break;
                        }
                    }
                }
            });
        }

        // If WebSocket is configured, spawn a separate task for the WebSocket listener
        if let Some((ws_listener, ws_config)) = self.websocket_listener {
            let matrix_ws = Arc::clone(&matrix);
            let registry_ws = Arc::clone(&registry);
            let db_ws = db.clone();
            let allow_origins = ws_config.allow_origins;
            let mut shutdown_rx_ws = matrix.shutdown_tx.subscribe();

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        result = ws_listener.accept() => {
                            let Ok((stream, addr)) = result.inspect_err(|e| {
                                error!(error = %e, "Failed to accept WebSocket connection");
                            }) else { continue };

                            let Some(uid) = validate_connection(&addr, &matrix_ws, "WebSocket") else {
                                continue;
                            };

                            if !matrix_ws.rate_limiter.on_connection_start(addr.ip()) {
                                warn!(%addr, "Connection rejected: max connections per IP exceeded");
                                continue;
                            }

                            tokio::spawn(handle_websocket_connection(
                                uid,
                                stream,
                                addr,
                                allow_origins.clone(),
                                Arc::clone(&matrix_ws),
                                Arc::clone(&registry_ws),
                                db_ws.clone(),
                            ));
                        }
                        _ = shutdown_rx_ws.recv() => {
                            info!("Shutdown signal received - stopping WebSocket listener");
                            break;
                        }
                    }
                }
            });
        }

        // Main plaintext listener loop
        loop {
            tokio::select! {
                // Handle incoming connections
                result = self.plaintext_listener.accept() => {
                    let Ok((stream, addr)) = result else {
                        if let Err(e) = result {
                            error!(error = %e, "Failed to accept plaintext connection");
                        }
                        continue;
                    };

                    let Some(uid) = validate_connection(&addr, &matrix, "Plaintext") else {
                        continue;
                    };

                    if !matrix.rate_limiter.on_connection_start(addr.ip()) {
                        warn!(%addr, "Connection rejected: max connections per IP exceeded");
                        continue;
                    }

                    tokio::spawn(handle_plaintext_connection(
                        uid,
                        stream,
                        addr,
                        Arc::clone(&matrix),
                        Arc::clone(&registry),
                        self.db.clone(),
                    ));
                }
                // Handle shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received - stopping gateway");
                    break;
                }
            }
        }

        Ok(())
    }
}
