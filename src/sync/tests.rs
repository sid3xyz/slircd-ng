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

    let mut machine1 = HandshakeMachine::new(sid1.clone(), "server1".to_string(), "Server 1".to_string());
    let mut machine2 = HandshakeMachine::new(sid2.clone(), "server2".to_string(), "Server 2".to_string());

    // Machine 1 initiates (Outbound)
    machine1.transition(HandshakeState::OutboundInitiated);

    // Machine 2 receives (Inbound)
    machine2.transition(HandshakeState::InboundReceived);

    // 1 sends PASS and SERVER (simulated)
    let pass1 = Command::Raw("PASS".to_string(), vec!["secret".to_string(), "TS=6".to_string(), "001".to_string()]);
    let server1 = Command::SERVER("server1".to_string(), 1, "001".to_string(), "Server 1".to_string());

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
