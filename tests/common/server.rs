//! Test server management.
//!
//! Spawns and manages slircd-ng instances for integration testing.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;
use tokio::time::sleep;

/// A test server instance.
pub struct TestServer {
    child: Child,
    #[allow(dead_code)]
    config_path: PathBuf,
    port: u16,
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

[security]
cloak_secret = "TestSecret-2026-Secure!9X"
cloak_suffix = "test"
spam_detection_enabled = false

[security.rate_limits]
message_rate_per_second = 1000
connection_burst_per_ip = 1000
join_burst_per_client = 1000

[motd]
lines = ["Test Server"]

[[oper]]
name = "testop"
password = "testpass"
host = "*@*"
"#,
            port,
            data_dir.display()
        );

        std::fs::write(&config_path, config_content)?;

        // Build path to slircd binary (in workspace target dir)
        let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
        let binary_path = PathBuf::from(cargo_manifest_dir)
            .parent()
            .expect("No parent directory")
            .join("target/debug/slircd");

        // Spawn the server process
        let child = Command::new(&binary_path)
            .arg(config_path.to_str().unwrap())
            .spawn()?;

        let server = Self {
            child,
            config_path,
            port,
            data_dir,
        };

        // Wait for server to start listening
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

    /// Get the port the server is listening on.
    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the server address.
    pub fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
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
