//! CRDT (Conflict-free Replicated Data Types) primitives for distributed IRC state.
//!
//! **Status: Experimental/Future** - These types are provided as building blocks
//! for future distributed/federated IRC features. They are not currently used by
//! the single-server `slircd-ng` implementation.
//!
//! ## Intended Use Cases
//!
//! - Server-to-server state synchronization (server linking)
//! - Eventually-consistent channel membership across network partitions
//! - Conflict resolution for concurrent nick/channel changes
//!
//! ## Types
//!
//! - [`LamportClock`] - Logical clock for ordering events across servers
//! - [`GSet`] - Grow-only set (add-only, never remove) - useful for ban lists
//! - [`LwwRegister`] - Last-Writer-Wins register - useful for topic, modes
//! - [`ORSet`] - Observed-Remove set (supports add and remove) - useful for channel members
//!
//! ## Why CRDTs for IRC?
//!
//! Traditional IRC networks use timestamp-based conflict resolution which can lose
//! updates during netsplits. CRDTs provide mathematically-proven convergence:
//! all servers will reach the same state regardless of message ordering.
//!
//! ## References
//!
//! - Shapiro et al., "A comprehensive study of Convergent and Commutative Replicated Data Types"

mod clock;
mod gset;
mod lww;
mod orset;

pub use clock::LamportClock;
pub use gset::GSet;
pub use lww::LwwRegister;
pub use orset::ORSet;
