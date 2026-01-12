//! LendingStream trait definition.

use std::pin::Pin;
use std::task::{Context, Poll};

/// A lending stream trait for zero-copy iteration.
///
/// Unlike `futures::Stream`, this trait allows yielding borrowed data
/// that references the stream's internal buffer. This enables true
/// zero-copy parsing without heap allocations.
///
/// # Generic Associated Types
///
/// This trait uses GATs (Generic Associated Types) to express that the lifetime
/// of yielded items is tied to the borrow of `self`, not to a separate lifetime
/// parameter. GATs were stabilized in Rust 1.65.
///
/// # Stability
///
/// This trait is considered stable for use. The API may evolve in future
/// versions following semver guidelines.
pub trait LendingStream {
    /// The item type yielded by this stream, borrowing from `self`.
    type Item<'a>
    where
        Self: 'a;
    /// The error type that can occur when polling.
    type Error;

    /// Poll the stream for the next item.
    ///
    /// This works similarly to `futures::Stream::poll_next`, but the
    /// returned item borrows from `self`.
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Item<'_>, Self::Error>>>;
}
