use slirc_crdt::channel::ChannelCrdt;
use slirc_crdt::clock::{HybridTimestamp, ServerId};
use slirc_crdt::traits::Crdt;

#[test]
fn test_crdt_channel_mode_convergence() {
    let sid_a = ServerId::new("00A");
    let sid_b = ServerId::new("00B");
    let t0 = HybridTimestamp::new(100, 0, &sid_a);

    let mut chan_a = ChannelCrdt::new("#test".to_string(), t0);
    let mut chan_b = ChannelCrdt::new("#test".to_string(), t0);

    // User joins
    let uid = "user1";
    chan_a.members.join(uid.to_string(), t0);
    chan_b.members.join(uid.to_string(), t0);

    // A sets +o at T1
    let t1 = HybridTimestamp::new(101, 0, &sid_a);
    chan_a
        .members
        .get_modes_mut(uid)
        .unwrap()
        .op
        .update(true, t1);

    // B sets +v at T2
    let t2 = HybridTimestamp::new(102, 0, &sid_b);
    chan_b
        .members
        .get_modes_mut(uid)
        .unwrap()
        .voice
        .update(true, t2);

    // Merge B into A
    chan_a.merge(&chan_b);

    let modes = chan_a.members.get_modes(uid).unwrap();
    assert!(*modes.op.value(), "A should have +o");
    assert!(*modes.voice.value(), "A should have +v");

    // Merge A into B
    chan_b.merge(&chan_a);
    let modes_b = chan_b.members.get_modes(uid).unwrap();
    assert!(*modes_b.op.value(), "B should have +o");
    assert!(*modes_b.voice.value(), "B should have +v");
}

#[test]
fn test_crdt_user_convergence_lww() {
    use slirc_crdt::user::UserCrdt;

    let sid_a = ServerId::new("00A");
    let sid_b = ServerId::new("00B");
    let t0 = HybridTimestamp::new(100, 0, &sid_a);

    let mut user_a = UserCrdt::new(
        "user1".to_string(),
        "nick1".to_string(),
        "user".to_string(),
        "Real Name".to_string(),
        "host".to_string(),
        "host".to_string(),
        t0,
    );
    let mut user_b = UserCrdt::new(
        "user1".to_string(),
        "nick1".to_string(),
        "user".to_string(),
        "Real Name".to_string(),
        "host".to_string(),
        "host".to_string(),
        t0,
    );

    // A sets nick to "Alice" at T1
    let t1 = HybridTimestamp::new(101, 0, &sid_a);
    user_a.nick.update("Alice".to_string(), t1);

    // B sets nick to "Bob" at T2
    let t2 = HybridTimestamp::new(102, 0, &sid_b);
    user_b.nick.update("Bob".to_string(), t2);

    // Merge A into B (B should win because T2 > T1)
    user_b.merge(&user_a);
    assert_eq!(user_b.nick.value(), "Bob");

    // Merge B into A (A should accept B because T2 > T1)
    user_a.merge(&user_b);
    assert_eq!(user_a.nick.value(), "Bob");
}

#[test]
fn test_crdt_topic_convergence_lww() {
    use slirc_crdt::channel::{ChannelCrdt, TopicCrdt};

    let sid_a = ServerId::new("00A");
    let sid_b = ServerId::new("00B");
    let t0 = HybridTimestamp::new(100, 0, &sid_a);

    let mut chan_a = ChannelCrdt::new("#test".to_string(), t0);
    let mut chan_b = ChannelCrdt::new("#test".to_string(), t0);

    // A sets topic at T1
    let t1 = HybridTimestamp::new(101, 0, &sid_a);
    chan_a.topic.update(
        Some(TopicCrdt {
            text: "Topic A".to_string(),
            set_by: "A".to_string(),
            set_at: 101,
        }),
        t1,
    );

    // B sets topic at T2
    let t2 = HybridTimestamp::new(102, 0, &sid_b);
    chan_b.topic.update(
        Some(TopicCrdt {
            text: "Topic B".to_string(),
            set_by: "B".to_string(),
            set_at: 102,
        }),
        t2,
    );

    // Merge A into B (B wins)
    chan_b.merge(&chan_a);
    assert_eq!(chan_b.topic.value().as_ref().unwrap().text, "Topic B");

    // Merge B into A (B wins)
    chan_a.merge(&chan_b);
    assert_eq!(chan_a.topic.value().as_ref().unwrap().text, "Topic B");
}
