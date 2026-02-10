//! Test server management.
//!
//! Spawns and manages slircd-ng instances for integration testing.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;
use tokio::time::sleep;

use super::tls::{TlsClientConfig, TlsTestPaths, generate_tls_assets};

/// A test server instance.
pub struct TestServer {
    child: Child,
    port: u16,
    tls_port: Option<u16>,
    tls_paths: Option<TlsTestPaths>,
    data_dir: PathBuf,
}

impl TestServer {
    /// Spawn a new test server with the given configuration.
    pub async fn spawn(port: u16) -> anyhow::Result<Self> {
        // Create temporary directory for test data
        let data_dir = std::env::temp_dir().join(format!("slircd-test-{}", port));
        std::fs::create_dir_all(&data_dir)?;

        // Create minimal test configuration
        let config_path = data_dir.join("config.toml");
        let config_content = format!(
            r#"
[server]
name = "test.server"
network = "TestNet"
sid = "00T"
description = "Test IRC Server"
metrics_port = 0

[listen]
address = "127.0.0.1:{}"

[database]
path = "{}/test.db"

    [timeouts]
    registration_timeout = 2

[security]
cloak_secret = "TestSecret-2026-Secure!9X"
cloak_suffix = "test"
spam_detection_enabled = false
allow_plaintext_sasl_plain = true

[security.rate_limits]
message_rate_per_second = 1000
connection_burst_per_ip = 1000
join_burst_per_client = 1000
max_connections_per_ip = 200

[motd]
lines = ["Test Server"]

[multiclient]
enabled = true

[history]
enabled = true
backend = "redb"
path = "{}/history.db"

[[oper]]
name = "testop"
password = "testpass"
host = "*@*"
"#,
            port,
            data_dir.display(),
            data_dir.display()
        );

        std::fs::write(&config_path, config_content)?;

        // Build path to slircd binary (in workspace target dir)
        let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
        let binary_path = PathBuf::from(cargo_manifest_dir).join("target/debug/slircd");

        // Spawn the server process
        let child = Command::new(&binary_path)
            .arg(config_path.to_str().unwrap())
            .spawn()?;

        let server = Self {
            child,
            port,
            tls_port: None,
            tls_paths: None,
            data_dir,
        };

        // Wait for server to start listening
        server.wait_until_ready().await?;

        Ok(server)
    }

    /// Spawn a new test server with TLS enabled and client cert verification optional.
    pub async fn spawn_tls(port: u16, tls_port: u16) -> anyhow::Result<Self> {
        let data_dir = std::env::temp_dir().join(format!("slircd-test-{}", port));
        std::fs::create_dir_all(&data_dir)?;

        let tls_dir = data_dir.join("tls");
        let tls_paths = generate_tls_assets(&tls_dir)?;

        let config_path = data_dir.join("config.toml");
        let config_content = format!(
            r#"
[server]
name = "test.server"
network = "TestNet"
sid = "00T"
description = "Test IRC Server"
metrics_port = 0

[listen]
address = "127.0.0.1:{}"

[tls]
address = "127.0.0.1:{}"
cert_path = "{}"
key_path = "{}"
client_auth = "optional"
ca_path = "{}"

[database]
path = "{}/test.db"

    [timeouts]
    registration_timeout = 2

[security]
cloak_secret = "TestSecret-2026-Secure!9X"
cloak_suffix = "test"
spam_detection_enabled = false
allow_plaintext_sasl_plain = true

[security.rate_limits]
message_rate_per_second = 1000
connection_burst_per_ip = 1000
join_burst_per_client = 1000
max_connections_per_ip = 200

[motd]
lines = ["Test Server"]

[multiclient]
enabled = true

[history]
enabled = true
backend = "redb"
path = "{}/history.db"

[[oper]]
name = "testop"
password = "testpass"
host = "*@*"
"#,
            port,
            tls_port,
            tls_paths.server_cert_path.display(),
            tls_paths.server_key_path.display(),
            tls_paths.ca_path.display(),
            data_dir.display(),
            data_dir.display()
        );

        std::fs::write(&config_path, config_content)?;

        let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
        let binary_path = PathBuf::from(cargo_manifest_dir).join("target/debug/slircd");

        let child = Command::new(&binary_path)
            .arg(config_path.to_str().unwrap())
            .spawn()?;

        let server = Self {
            child,
            port,
            tls_port: Some(tls_port),
            tls_paths: Some(tls_paths),
            data_dir,
        };

        server.wait_until_ready().await?;

        Ok(server)
    }

    /// Wait until the server is accepting connections.
    async fn wait_until_ready(&self) -> anyhow::Result<()> {
        for _ in 0..30 {
            if tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .is_ok()
            {
                return Ok(());
            }
            sleep(Duration::from_millis(100)).await;
        }
        anyhow::bail!("Server failed to start within 3 seconds")
    }

    /// Get the server address.
    pub fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// Get the TLS address (if configured).
    pub fn tls_address(&self) -> Option<String> {
        self.tls_port.map(|port| format!("127.0.0.1:{}", port))
    }

    /// Create a new test client connected to this server.
    pub async fn connect(&self, nick: &str) -> anyhow::Result<super::client::TestClient> {
        super::client::TestClient::connect(&self.address(), nick).await
    }

    /// Create a new TLS test client connected to this server with a client certificate.
    pub async fn connect_tls_with_client_cert(
        &self,
        nick: &str,
    ) -> anyhow::Result<super::client::TestClient> {
        let tls_paths = self
            .tls_paths
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TLS is not configured for this test server"))?;
        let address = self
            .tls_address()
            .ok_or_else(|| anyhow::anyhow!("TLS is not configured for this test server"))?;
        let tls_config = TlsClientConfig::with_client_cert(tls_paths);
        super::client::TestClient::connect_tls(&address, nick, tls_config).await
    }

    /// Create a new TLS test client connected to this server without a client certificate.
    pub async fn connect_tls_without_client_cert(
        &self,
        nick: &str,
    ) -> anyhow::Result<super::client::TestClient> {
        let tls_paths = self
            .tls_paths
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TLS is not configured for this test server"))?;
        let address = self
            .tls_address()
            .ok_or_else(|| anyhow::anyhow!("TLS is not configured for this test server"))?;
        let tls_config = TlsClientConfig::without_client_cert(tls_paths);
        super::client::TestClient::connect_tls(&address, nick, tls_config).await
    }

    /// Spawn a test server with custom config path.
    /// Used for REHASH testing where we modify the config file on disk.
    pub async fn spawn_with_config(port: u16, config_path: PathBuf) -> anyhow::Result<Self> {
        // Verify config file exists
        if !config_path.exists() {
            anyhow::bail!("Config file not found: {:?}", config_path);
        }

        // Build path to slircd binary
        let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
        let binary_path = PathBuf::from(cargo_manifest_dir).join("target/debug/slircd");

        // Spawn the server process with insecure cloak bypass for testing
        let child = Command::new(&binary_path)
            .arg(config_path.to_str().unwrap())
            .env("SLIRCD_ALLOW_INSECURE_CLOAK", "1")
            .spawn()?;

        let server = Self {
            child,
            port,
            tls_port: None,
            tls_paths: None,
            data_dir: config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf(),
        };

        // Wait for server to start listening
        server.wait_until_ready().await?;

        Ok(server)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // Kill the server process
        let _ = self.child.kill();
        let _ = self.child.wait();

        // Clean up test data directory
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}
