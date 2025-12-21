#[cfg(test)]
mod tests {
    use crate::config::{
        AccountRegistrationConfig, Config, HistoryConfig, IdleTimeoutsConfig, LimitsConfig,
        ListenConfig, MotdConfig, SecurityConfig, ServerConfig,
    };
    use crate::db::Database;
    use crate::state::actor::ChannelEvent;
    use crate::state::{Matrix, MatrixParams};
    use crate::sync::SyncManager;
    use crate::sync::protocol::IncomingCommandHandler;
    use slirc_crdt::clock::ServerId;
    use slirc_proto::{Command, Message};
    use std::sync::Arc;
    use std::time::Instant;
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

        let manager = SyncManager::new(
            ServerId::new("001".to_string()),
            "test.server".to_string(),
            "Test Server".to_string(),
            vec![],
        );
        let peer_sid = ServerId::new("002".to_string());
        // Mock link
        let (tx, _rx) = mpsc::channel(100);
        manager.links.insert(
            peer_sid.clone(),
            crate::sync::LinkState {
                tx,
                state: crate::sync::handshake::HandshakeState::Synced,
                name: "peer.server".to_string(),
                last_pong: Instant::now(),
                last_ping: Instant::now(),
            },
        );

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
        handler
            .handle_message(Message::from(uid_cmd), &manager, &peer_sid)
            .await
            .unwrap();

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
        handler
            .handle_message(Message::from(sjoin_cmd), &manager, &peer_sid)
            .await
            .unwrap();

        // Verify channel exists
        assert!(matrix.channel_manager.channels.contains_key("#test"));

        // Verify user's channel list (updated synchronously in handle_sjoin)
        let user_arc = matrix.user_manager.users.get(&uid.unwrap()).unwrap();
        let user_guard = user_arc.read().await;
        assert!(user_guard.channels.contains("#test"));
    }

    #[tokio::test]
    async fn test_ping_pong() {
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

        let manager = SyncManager::new(
            ServerId::new("001".to_string()),
            "test.server".to_string(),
            "Test Server".to_string(),
            vec![],
        );

        // Register a fake peer
        let peer_sid = ServerId::new("002".to_string());
        let (tx, mut rx) = mpsc::channel(100);
        manager.links.insert(
            peer_sid.clone(),
            crate::sync::LinkState {
                tx,
                state: crate::sync::handshake::HandshakeState::Synced,
                name: "peer.server".to_string(),
                last_pong: Instant::now(),
                last_ping: Instant::now(),
            },
        );

        // Test PING
        let ping_cmd = Command::PING("peer.server".to_string(), None);
        handler
            .handle_message(Message::from(ping_cmd), &manager, &peer_sid)
            .await
            .unwrap();

        // Expect PONG
        let msg = rx.recv().await.unwrap();
        if let Command::PONG(origin, target) = msg.command {
            assert_eq!(origin, "001");
            assert_eq!(target, Some("peer.server".to_string()));
        } else {
            panic!("Expected PONG");
        }

        // Test PONG updates timestamp
        let old_pong = manager.links.get(&peer_sid).unwrap().last_pong;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let pong_cmd = Command::PONG("peer.server".to_string(), Some("001".to_string()));
        handler
            .handle_message(Message::from(pong_cmd), &manager, &peer_sid)
            .await
            .unwrap();

        let new_pong = manager.links.get(&peer_sid).unwrap().last_pong;
        assert!(new_pong > old_pong);
    }

    #[tokio::test]
    async fn test_loop_detection() {
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

        let manager = SyncManager::new(
            ServerId::new("001".to_string()),
            "test.server".to_string(),
            "Test Server".to_string(),
            vec![],
        );

        // Register a fake peer
        let peer_sid = ServerId::new("002".to_string());
        let (tx, mut rx) = mpsc::channel(100);
        manager.links.insert(
            peer_sid.clone(),
            crate::sync::LinkState {
                tx,
                state: crate::sync::handshake::HandshakeState::Synced,
                name: "peer.server".to_string(),
                last_pong: Instant::now(),
                last_ping: Instant::now(),
            },
        );

        // Add "003" to topology
        let sid3 = ServerId::new("003".to_string());
        manager.topology.servers.insert(
            sid3.clone(),
            crate::sync::ServerInfo {
                sid: sid3.clone(),
                name: "server3".to_string(),
                info: "Server 3".to_string(),
                hopcount: 1,
                via: Some(peer_sid.clone()),
            },
        );

        // Try to introduce "003" again via SID command
        let sid_cmd = Command::SID(
            "server3".to_string(),
            "2".to_string(),
            "003".to_string(),
            "Server 3".to_string(),
        );
        let result = handler
            .handle_message(Message::from(sid_cmd), &manager, &peer_sid)
            .await;

        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "Loop detected");

        // Expect ERROR command sent to peer
        let msg = rx.recv().await.unwrap();
        if let Command::ERROR(msg) = msg.command {
            assert!(msg.contains("Loop detected"));
        } else {
            panic!("Expected ERROR command");
        }
    }

    #[tokio::test]
    async fn test_operational_commands() {
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

        let manager = SyncManager::new(
            ServerId::new("001".to_string()),
            "test.server".to_string(),
            "Test Server".to_string(),
            vec![],
        );
        let peer_sid = ServerId::new("002".to_string());
        // Mock link
        let (tx, _rx) = mpsc::channel(100);
        manager.links.insert(
            peer_sid.clone(),
            crate::sync::LinkState {
                tx,
                state: crate::sync::handshake::HandshakeState::Synced,
                name: "peer.server".to_string(),
                last_pong: Instant::now(),
                last_ping: Instant::now(),
            },
        );

        // 1. Create a user and channel
        let uid = "001AAAAAA".to_string();
        let session_id = uuid::Uuid::new_v4();
        let user = crate::state::User {
            uid: uid.clone(),
            nick: "testuser".to_string(),
            user: "user".to_string(),
            realname: "Real Name".to_string(),
            host: "host".to_string(),
            ip: "127.0.0.1".to_string(),
            visible_host: "host".to_string(),
            session_id,
            channels: std::collections::HashSet::new(),
            modes: crate::state::UserModes::default(),
            account: None,
            away: None,
            caps: std::collections::HashSet::new(),
            certfp: None,
            silence_list: std::collections::HashSet::new(),
            accept_list: std::collections::HashSet::new(),
            last_modified: slirc_crdt::clock::HybridTimestamp::now(&matrix.server_id),
        };
        matrix.user_manager.add_local_user(user).await;

        let channel_name = "#test".to_string();
        let chan_tx = matrix
            .channel_manager
            .get_or_create_actor(channel_name.clone(), Arc::downgrade(&matrix))
            .await;

        // Join user to channel
        let (join_tx, join_rx) = tokio::sync::oneshot::channel();
        let join_params = Box::new(crate::state::actor::JoinParams {
            uid: uid.clone(),
            nick: "testuser".to_string(),
            sender: mpsc::channel(1).0, // Dummy
            caps: std::collections::HashSet::new(),
            user_context: crate::security::UserContext {
                nickname: "testuser".to_string(),
                username: "user".to_string(),
                hostname: "host".to_string(),
                realname: "Real Name".to_string(),
                account: None,
                server: "test.server".to_string(),
                channels: vec![],
                is_oper: false,
                oper_type: None,
                certificate_fp: None,
                sasl_mechanism: None,
                is_registered: false,
                is_tls: false,
            },
            key: None,
            initial_modes: None,
            join_msg_extended: slirc_proto::Message::from(Command::JOIN(
                channel_name.clone(),
                None,
                None,
            )),
            join_msg_standard: slirc_proto::Message::from(Command::JOIN(
                channel_name.clone(),
                None,
                None,
            )),
            session_id,
        });
        chan_tx
            .send(ChannelEvent::Join {
                params: join_params,
                reply_tx: join_tx,
            })
            .await
            .unwrap();
        join_rx.await.unwrap().unwrap();

        // 2. TMODE
        let tmode_cmd = Command::TMODE(
            100, // TS
            channel_name.clone(),
            "+o".to_string(),
            vec![uid.clone()],
        );
        let msg = slirc_proto::Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new_from_str("002")),
            command: tmode_cmd,
        };
        handler
            .handle_message(msg, &manager, &peer_sid)
            .await
            .unwrap();

        // Verify op status
        let (mode_tx, mode_rx) = tokio::sync::oneshot::channel();
        chan_tx
            .send(ChannelEvent::GetMemberModes {
                uid: uid.clone(),
                reply_tx: mode_tx,
            })
            .await
            .unwrap();
        let modes = mode_rx.await.unwrap().unwrap();
        assert!(modes.op, "User should be op after TMODE");

        // 3. TOPIC
        let topic_cmd = Command::TOPIC(
            channel_name.clone(),
            Some("1234567890 New Topic".to_string()),
        );
        let msg = slirc_proto::Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new_from_str("002")),
            command: topic_cmd,
        };
        handler
            .handle_message(msg, &manager, &peer_sid)
            .await
            .unwrap();

        // Verify topic
        let (info_tx, info_rx) = tokio::sync::oneshot::channel();
        chan_tx
            .send(ChannelEvent::GetInfo {
                requester_uid: None,
                reply_tx: info_tx,
            })
            .await
            .unwrap();
        let info = info_rx.await.unwrap();
        assert_eq!(info.topic.unwrap().text, "New Topic");

        // 4. KICK
        let kick_cmd = Command::KICK(channel_name.clone(), uid.clone(), Some("Bye".to_string()));
        let msg = slirc_proto::Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new_from_str("002")),
            command: kick_cmd,
        };
        handler
            .handle_message(msg, &manager, &peer_sid)
            .await
            .unwrap();

        // Verify kick
        let (mem_tx, mem_rx) = tokio::sync::oneshot::channel();
        chan_tx
            .send(ChannelEvent::GetMembers { reply_tx: mem_tx })
            .await
            .unwrap();
        let members = mem_rx.await.unwrap();
        assert!(!members.contains_key(&uid), "User should be kicked");
    }
}
