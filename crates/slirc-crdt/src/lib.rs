//! Conflict-free Replicated Data Types (CRDTs) for distributed IRC state.
//!
//! This crate provides CRDT implementations for synchronizing IRC server state
//! across multiple linked servers without coordination. Each data type is designed
//! for eventual consistency with deterministic conflict resolution.
//!
//! # Architecture
//!
//! The CRDT layer sits between the domain managers and the network layer:
//!
//! ```text
//! ┌─────────────────────┐     ┌─────────────────────┐
//! │   UserManager       │     │   ChannelManager    │
//! │   (local state)     │     │   (local state)     │
//! └──────────┬──────────┘     └──────────┬──────────┘
//!            │                           │
//!            ▼                           ▼
//! ┌──────────────────────────────────────────────────┐
//! │              CRDT Layer (this crate)             │
//! │  ┌──────────────┐  ┌───────────────────────┐    │
//! │  │  UserCrdt    │  │   ChannelCrdt         │    │
//! │  │  (LWW/Union) │  │   (LWW/AWSet)         │    │
//! │  └──────────────┘  └───────────────────────┘    │
//! └──────────────────────────────────────────────────┘
//!            │                           │
//!            ▼                           ▼
//! ┌──────────────────────────────────────────────────┐
//! │         Server Linking Protocol                   │
//! │         (gossip / anti-entropy)                   │
//! └──────────────────────────────────────────────────┘
//! ```
//!
//! # CRDT Types Used
//!
//! - **LWW (Last-Writer-Wins)**: For scalar values like nicknames, away messages.
//! - **`AWSet` (Add-Wins Set)**: For collections where adds should take precedence.
//! - **`ORSet` (Observed-Remove Set)**: For collections where concurrent add/remove
//!   should both succeed (like channel membership).
//! - **Vector Clock**: For causal ordering of events across servers.

pub mod channel;
pub mod clock;
pub mod traits;
pub mod user;

pub use channel::ChannelCrdt;
pub use clock::{HybridTimestamp, ServerId, VectorClock};
pub use traits::{Crdt, Mergeable, StateDelta};
pub use user::UserCrdt;

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify all public re-exports are accessible.
    #[test]
    fn test_public_reexports() {
        // Clock types
        let server = ServerId::new("001");
        let _ts = HybridTimestamp::now(&server);
        let _vc = VectorClock::new();

        // Channel CRDT
        let ts = HybridTimestamp::new(100, 0, &server);
        let _chan = ChannelCrdt::new("#test".to_string(), ts);

        // User CRDT
        let _user = UserCrdt::new(
            "001AAA".to_string(),
            "Nick".to_string(),
            "user".to_string(),
            "Real".to_string(),
            "host".to_string(),
            "cloak".to_string(),
            ts,
        );
    }

    /// Verify Crdt trait is usable with concrete types.
    #[test]
    fn test_crdt_trait_bounds() {
        fn assert_crdt<T: Crdt>(_: &T) {}

        let server = ServerId::new("001");
        let ts = HybridTimestamp::new(100, 0, &server);

        let chan = ChannelCrdt::new("#test".to_string(), ts);
        assert_crdt(&chan);

        let user = UserCrdt::new(
            "001AAA".to_string(),
            "Nick".to_string(),
            "user".to_string(),
            "Real".to_string(),
            "host".to_string(),
            "cloak".to_string(),
            ts,
        );
        assert_crdt(&user);
    }

    /// Verify Mergeable trait is usable.
    #[test]
    fn test_mergeable_trait() {
        fn assert_mergeable<T: Mergeable>(_: &T) {}

        let server = ServerId::new("001");
        let ts = HybridTimestamp::new(100, 0, &server);
        let reg = traits::LwwRegister::new("value", ts);
        assert_mergeable(&reg);
    }
}
