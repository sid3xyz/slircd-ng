//! Gateway - TCP listener that accepts incoming connections.
//!
//! The Gateway binds to a socket and spawns Connection tasks for each
//! incoming client.

use crate::handlers::Registry;
use crate::network::Connection;
use crate::state::Matrix;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info, instrument};

/// The Gateway accepts incoming TCP connections and spawns handlers.
pub struct Gateway {
    listener: TcpListener,
    matrix: Arc<Matrix>,
    registry: Arc<Registry>,
}

impl Gateway {
    /// Bind the gateway to the specified address.
    pub async fn bind(addr: SocketAddr, matrix: Arc<Matrix>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let registry = Arc::new(Registry::new());
        info!(%addr, "Gateway listening");
        Ok(Self { listener, matrix, registry })
    }

    /// Run the gateway, accepting connections forever.
    #[instrument(skip(self), name = "gateway")]
    pub async fn run(self) -> std::io::Result<()> {
        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    info!(%addr, "Connection accepted");

                    let matrix = Arc::clone(&self.matrix);
                    let registry = Arc::clone(&self.registry);
                    let uid = matrix.uid_gen.next();

                    tokio::spawn(async move {
                        let connection = Connection::new(uid.clone(), stream, addr, matrix, registry);
                        if let Err(e) = connection.run().await {
                            error!(%uid, %addr, error = %e, "Connection error");
                        }
                        info!(%uid, %addr, "Connection closed");
                    });
                }
                Err(e) => {
                    error!(error = %e, "Failed to accept connection");
                }
            }
        }
    }
}
