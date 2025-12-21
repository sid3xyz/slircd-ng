use crate::db::Database;
use crate::handlers::Registry;
use crate::state::Matrix;
use slirc_proto::Message;
use slirc_proto::transport::ZeroCopyTransportEnum;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;

/// Shared connection resources used across lifecycle phases.
///
/// Groups parameters that are common to both handshake and event loop,
/// reducing function signature complexity.
pub struct ConnectionContext<'a> {
    /// The user's unique identifier.
    pub uid: &'a str,
    /// Transport for reading/writing IRC messages.
    pub transport: &'a mut ZeroCopyTransportEnum,
    /// Shared server state (users, channels, config).
    pub matrix: &'a Arc<Matrix>,
    /// Command handler registry.
    pub registry: &'a Arc<Registry>,
    /// Database for persistence (accounts, bans).
    pub db: &'a Database,
    /// Client's remote address.
    pub addr: SocketAddr,
    /// TLS acceptor for STARTTLS upgrade (only on plaintext connections).
    pub starttls_acceptor: Option<&'a TlsAcceptor>,
}

/// Message channels for lifecycle phases.
pub struct LifecycleChannels<'a> {
    /// Sender for queueing outgoing messages.
    pub tx: &'a mpsc::Sender<Message>,
    /// Receiver for draining outgoing messages.
    pub rx: &'a mut mpsc::Receiver<Message>,
}
