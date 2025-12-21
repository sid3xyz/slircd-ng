use super::handshake::{HandshakeMachine, HandshakeState};
use crate::config::LinkBlock;
use slirc_crdt::clock::ServerId;
use slirc_proto::Command;

fn create_link(name: &str, password: &str) -> LinkBlock {
    LinkBlock {
        name: name.to_string(),
        hostname: "localhost".to_string(),
        port: 6667,
        password: password.to_string(),
        tls: false,
        verify_cert: true,
        autoconnect: false,
        sid: None,
    }
}

#[test]
fn test_handshake_flow() {
    let sid1 = ServerId::new("001".to_string());
    let sid2 = ServerId::new("002".to_string());

    let link1 = create_link("server2", "secret");
    let link2 = create_link("server1", "secret");

    let mut machine1 =
        HandshakeMachine::new(sid1.clone(), "server1".to_string(), "Server 1".to_string());
    let mut machine2 =
        HandshakeMachine::new(sid2.clone(), "server2".to_string(), "Server 2".to_string());

    // Machine 1 initiates (Outbound)
    machine1.transition(HandshakeState::OutboundInitiated);

    // Machine 2 receives (Inbound)
    machine2.transition(HandshakeState::InboundReceived);

    // 1 sends PASS and SERVER (simulated)
    let pass1 = Command::Raw(
        "PASS".to_string(),
        vec!["secret".to_string(), "TS=6".to_string(), "001".to_string()],
    );
    let server1 = Command::SERVER(
        "server1".to_string(),
        1,
        "001".to_string(),
        "Server 1".to_string(),
    );

    // 2 processes PASS
    let res = machine2.step(pass1, &[link2.clone()]).unwrap();
    assert!(res.is_empty());

    // 2 processes SERVER
    let res = machine2.step(server1, &[link2.clone()]).unwrap();
    assert_eq!(machine2.state, HandshakeState::Bursting);
    assert_eq!(res.len(), 2); // Should send PASS and SERVER back

    let pass2 = res[0].clone();
    let server2 = res[1].clone();

    // 1 processes PASS from 2
    let res = machine1.step(pass2, &[link1.clone()]).unwrap();
    assert!(res.is_empty());

    // 1 processes SERVER from 2
    let res = machine1.step(server2, &[link1.clone()]).unwrap();
    assert_eq!(machine1.state, HandshakeState::Bursting);
    assert!(res.is_empty());
}

#[tokio::test]
async fn test_sync_manager_peer_registration() {
    use super::SyncManager;

    let sid = ServerId::new("001".to_string());
    let sync = SyncManager::new(
        sid,
        "test.server".to_string(),
        "Test Server".to_string(),
        vec![],
    );

    // Register a peer
    let peer_sid = ServerId::new("002".to_string());
    let _rx = sync
        .register_peer(
            peer_sid.clone(),
            "peer.server".to_string(),
            1,
            "Peer Server".to_string(),
        )
        .await;

    // Verify peer is registered
    assert!(sync.links.contains_key(&peer_sid));
    assert!(sync.topology.servers.contains_key(&peer_sid));

    // Verify we can find the peer
    let link = sync.get_peer_for_server(&peer_sid);
    assert!(link.is_some());

    // Remove peer
    sync.remove_peer(&peer_sid).await;
    assert!(!sync.links.contains_key(&peer_sid));
}

#[tokio::test]
async fn test_state_observer_split_horizon() {
    use super::SyncManager;
    use crate::state::observer::StateObserver;
    use slirc_crdt::channel::{ChannelCrdt, ChannelModesCrdt, MembershipCrdt};
    use slirc_crdt::clock::HybridTimestamp;
    use slirc_crdt::traits::AwSet;

    let sid = ServerId::new("001".to_string());
    let sync = SyncManager::new(
        sid.clone(),
        "test.server".to_string(),
        "Test Server".to_string(),
        vec![],
    );

    // Register two peers
    let peer1_sid = ServerId::new("002".to_string());
    let mut rx1 = sync
        .register_peer(
            peer1_sid.clone(),
            "peer1.server".to_string(),
            1,
            "Peer 1".to_string(),
        )
        .await;

    let peer2_sid = ServerId::new("003".to_string());
    let mut rx2 = sync
        .register_peer(
            peer2_sid.clone(),
            "peer2.server".to_string(),
            1,
            "Peer 2".to_string(),
        )
        .await;

    // Create a channel update
    let ts = HybridTimestamp::new(1, 0, &sid);
    let channel = ChannelCrdt {
        name: "#test".to_string(),
        modes: ChannelModesCrdt::new(ts),
        topic: slirc_crdt::traits::LwwRegister::new(None, ts),
        key: slirc_crdt::traits::LwwRegister::new(None, ts),
        limit: slirc_crdt::traits::LwwRegister::new(None, ts),
        created_at: ts,
        members: MembershipCrdt::new(),
        bans: AwSet::new(),
        excepts: AwSet::new(),
        invites: AwSet::new(),
    };

    // Notify with NO source (local change) - should broadcast to all peers
    sync.on_channel_update(&channel, None);

    // Give async task time to send
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Both peers should receive
    let msg1 = rx1.try_recv();
    let msg2 = rx2.try_recv();
    assert!(msg1.is_ok(), "Peer 1 should receive channel update");
    assert!(msg2.is_ok(), "Peer 2 should receive channel update");

    // Verify SJOIN command was sent
    let m1 = msg1.unwrap();
    assert!(
        matches!(m1.command, Command::SJOIN(..)),
        "Should be SJOIN command"
    );
}

#[tokio::test]
async fn test_state_observer_skip_source() {
    use super::SyncManager;
    use crate::state::observer::StateObserver;
    use slirc_crdt::channel::{ChannelCrdt, ChannelModesCrdt, MembershipCrdt};
    use slirc_crdt::clock::HybridTimestamp;
    use slirc_crdt::traits::AwSet;

    let sid = ServerId::new("001".to_string());
    let sync = SyncManager::new(
        sid.clone(),
        "test.server".to_string(),
        "Test Server".to_string(),
        vec![],
    );

    // Register a peer
    let peer_sid = ServerId::new("002".to_string());
    let mut rx = sync
        .register_peer(
            peer_sid.clone(),
            "peer.server".to_string(),
            1,
            "Peer".to_string(),
        )
        .await;

    // Create a channel update from the peer (source = Some(peer_sid))
    let ts = HybridTimestamp::new(1, 0, &sid);
    let channel = ChannelCrdt {
        name: "#test".to_string(),
        modes: ChannelModesCrdt::new(ts),
        topic: slirc_crdt::traits::LwwRegister::new(None, ts),
        key: slirc_crdt::traits::LwwRegister::new(None, ts),
        limit: slirc_crdt::traits::LwwRegister::new(None, ts),
        created_at: ts,
        members: MembershipCrdt::new(),
        bans: AwSet::new(),
        excepts: AwSet::new(),
        invites: AwSet::new(),
    };

    // Notify WITH source (remote change) - should NOT broadcast back
    sync.on_channel_update(&channel, Some(peer_sid));

    // Give async task time to send (if it were to)
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Peer should NOT receive (split horizon)
    let msg = rx.try_recv();
    assert!(
        msg.is_err(),
        "Peer should NOT receive update from itself (split-horizon)"
    );
}
