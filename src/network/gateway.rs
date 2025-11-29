//! Gateway - TCP/TLS listener that accepts incoming connections.
//!
//! The Gateway binds to sockets and spawns Connection tasks for each
//! incoming client. Supports both plaintext and TLS connections.

use crate::config::TlsConfig;
use crate::db::Database;
use crate::handlers::Registry;
use crate::network::Connection;
use crate::state::Matrix;
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::io::{BufReader, Cursor};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, instrument, warn};

/// The Gateway accepts incoming TCP/TLS connections and spawns handlers.
pub struct Gateway {
    plaintext_listener: TcpListener,
    tls_listener: Option<(TcpListener, TlsAcceptor)>,
    matrix: Arc<Matrix>,
    registry: Arc<Registry>,
    db: Database,
}

impl Gateway {
    /// Bind the gateway to the specified addresses.
    pub async fn bind(
        addr: SocketAddr,
        tls_config: Option<TlsConfig>,
        matrix: Arc<Matrix>,
        db: Database,
    ) -> anyhow::Result<Self> {
        let plaintext_listener = TcpListener::bind(addr).await?;
        let registry = Arc::new(Registry::new());
        info!(%addr, "Plaintext listener bound");

        let tls_listener = if let Some(tls_cfg) = tls_config {
            let tls_acceptor = Self::load_tls(&tls_cfg)?;
            let listener = TcpListener::bind(tls_cfg.address).await?;
            info!(address = %tls_cfg.address, "TLS listener bound");
            Some((listener, tls_acceptor))
        } else {
            None
        };

        Ok(Self {
            plaintext_listener,
            tls_listener,
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
        let certs: Vec<CertificateDer> = certs(cert_reader)
            .collect::<Result<Vec<_>, _>>()?;

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

        // Build TLS server config
        let tls_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        Ok(TlsAcceptor::from(Arc::new(tls_config)))
    }

    /// Run the gateway, accepting connections forever.
    #[instrument(skip(self), name = "gateway")]
    pub async fn run(self) -> anyhow::Result<()> {
        let matrix = Arc::clone(&self.matrix);
        let registry = Arc::clone(&self.registry);
        let db = self.db.clone();

        // If TLS is configured, spawn a separate task for the TLS listener
        if let Some((tls_listener, tls_acceptor)) = self.tls_listener {
            let matrix_tls = Arc::clone(&matrix);
            let registry_tls = Arc::clone(&registry);
            let db_tls = db.clone();

            tokio::spawn(async move {
                loop {
                    match tls_listener.accept().await {
                        Ok((stream, addr)) => {
                            info!(%addr, "TLS connection accepted");

                            let matrix = Arc::clone(&matrix_tls);
                            let registry = Arc::clone(&registry_tls);
                            let db = db_tls.clone();
                            let uid = matrix.uid_gen.next();
                            let acceptor = tls_acceptor.clone();

                            tokio::spawn(async move {
                                // Perform TLS handshake
                                match acceptor.accept(stream).await {
                                    Ok(tls_stream) => {
                                        let connection = Connection::new_tls(
                                            uid.clone(),
                                            tls_stream,
                                            addr,
                                            matrix,
                                            registry,
                                            db,
                                        );
                                        if let Err(e) = connection.run().await {
                                            error!(%uid, %addr, error = %e, "TLS connection error");
                                        }
                                        info!(%uid, %addr, "TLS connection closed");
                                    }
                                    Err(e) => {
                                        warn!(%addr, error = %e, "TLS handshake failed");
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to accept TLS connection");
                        }
                    }
                }
            });
        }

        // Main plaintext listener loop
        loop {
            match self.plaintext_listener.accept().await {
                Ok((stream, addr)) => {
                    info!(%addr, "Plaintext connection accepted");

                    let matrix = Arc::clone(&matrix);
                    let registry = Arc::clone(&registry);
                    let db = self.db.clone();
                    let uid = matrix.uid_gen.next();

                    tokio::spawn(async move {
                        let connection = Connection::new_plaintext(
                            uid.clone(),
                            stream,
                            addr,
                            matrix,
                            registry,
                            db,
                        );
                        if let Err(e) = connection.run().await {
                            error!(%uid, %addr, error = %e, "Plaintext connection error");
                        }
                        info!(%uid, %addr, "Plaintext connection closed");
                    });
                }
                Err(e) => {
                    error!(error = %e, "Failed to accept plaintext connection");
                }
            }
        }
    }
}
