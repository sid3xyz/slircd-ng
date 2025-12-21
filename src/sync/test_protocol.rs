
#[cfg(test)]
mod tests {
    use crate::state::{Matrix, MatrixParams};
    use crate::sync::protocol::IncomingCommandHandler;
    use crate::config::{Config, ServerConfig, ListenConfig, HistoryConfig, SecurityConfig, AccountRegistrationConfig, MotdConfig, LimitsConfig, IdleTimeoutsConfig};
    use crate::db::Database;
    use slirc_proto::Command;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn create_test_config() -> Config {
        Config {
            server: ServerConfig {
                name: "test.server".to_string(),
                network: "TestNet".to_string(),
                sid: "001".to_string(),
                description: "Test Server".to_string(),
                password: None,
                metrics_port: None,
                admin_info1: None,
                admin_info2: None,
                admin_email: None,
                idle_timeouts: IdleTimeoutsConfig::default(),
            },
            listen: ListenConfig {
                address: "127.0.0.1:6667".parse().unwrap(),
            },
            tls: None,
            websocket: None,
            oper: vec![],
            webirc: vec![],
            database: None,
            history: HistoryConfig::default(),
            security: SecurityConfig::default(),
            account_registration: AccountRegistrationConfig::default(),
            motd: MotdConfig::default(),
            limits: LimitsConfig::default(),
            links: vec![],
        }
    }

    #[tokio::test]
    async fn test_incoming_sjoin() {
        let config = create_test_config();
        let db = Database::new(":memory:").await.unwrap();
        let (disconnect_tx, _) = mpsc::channel(100);
        let history = Arc::new(crate::history::noop::NoOpProvider);

        let params = MatrixParams {
            config: &config,
            data_dir: Some(std::path::Path::new(".")),
            db,
            history,
            registered_channels: vec![],
            shuns: vec![],
            klines: vec![],
            dlines: vec![],
            glines: vec![],
            zlines: vec![],
            disconnect_tx,
        };

        let (matrix, _) = Matrix::new(params);
        let matrix = Arc::new(matrix);
        let handler = IncomingCommandHandler::new(matrix.clone());

        // 1. Create a user via UID
        let uid_cmd = Command::UID(
            "testuser".to_string(),
            "1".to_string(),
            "1234567890".to_string(),
            "user".to_string(),
            "host".to_string(),
            "001AAAAAA".to_string(),
            "+i".to_string(),
            "Real Name".to_string(),
        );
        handler.handle_command(uid_cmd).await.unwrap();

        // Verify user exists
        let uid = matrix.user_manager.nicks.get("testuser").map(|u| u.clone());
        assert!(uid.is_some());

        // 2. Send SJOIN
        let sjoin_cmd = Command::SJOIN(
            1234567890,
            "#test".to_string(),
            "+nt".to_string(),
            vec![],
            vec![("@".to_string(), "001AAAAAA".to_string())],
        );
        handler.handle_command(sjoin_cmd).await.unwrap();

        // Verify channel exists
        assert!(matrix.channel_manager.channels.contains_key("#test"));

        // Verify user's channel list (updated synchronously in handle_sjoin)
        let user_arc = matrix.user_manager.users.get(&uid.unwrap()).unwrap();
        let user_guard = user_arc.read().await;
        assert!(user_guard.channels.contains("#test"));
    }
}
