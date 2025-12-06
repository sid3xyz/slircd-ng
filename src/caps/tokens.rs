//! Core capability token types.
//!
//! This module defines the unforgeable `Cap<T>` token and the `Capability` trait.

use std::fmt;
use std::marker::PhantomData;

/// An unforgeable capability token proving authorization.
///
/// This type can only be constructed within the `caps` module (via `pub(super)`),
/// ensuring that only [`CapabilityAuthority`](super::authority::CapabilityAuthority)
/// can mint tokens.
///
/// # Security Properties
///
/// - **Unforgeable**: `new()` is `pub(super)`, only Authority can create
/// - **Non-transferable**: `!Clone` and `!Copy` prevent token sharing
/// - **Scoped**: Contains the resource scope (e.g., channel name)
/// - **Typed**: Generic parameter prevents mixing capability types
///
/// # Example
///
/// ```ignore
/// // Only CapabilityAuthority can create:
/// let cap: Cap<KickCap> = authority.request_kick_cap(uid, "#channel").await?;
///
/// // The cap proves authorization for the scoped resource
/// assert_eq!(cap.scope(), "#channel");
///
/// // Can't clone or copy - must pass ownership
/// channel.kick(target, cap); // cap is moved
/// ```
pub struct Cap<T: Capability> {
    /// The resource this capability is scoped to.
    scope: T::Scope,
    /// Zero-sized marker for the capability type.
    _marker: PhantomData<T>,
}

// Explicitly NOT deriving Clone, Copy, or Default to prevent token leakage.
// These are security-critical non-implementations.

impl<T: Capability> Cap<T> {
    /// Create a new capability token.
    ///
    /// # Safety
    ///
    /// This is `pub(super)` to ensure only `CapabilityAuthority` can mint tokens.
    /// Direct construction from outside the `caps` module is a compile error.
    #[inline]
    pub(super) fn new(scope: T::Scope) -> Self {
        Self {
            scope,
            _marker: PhantomData,
        }
    }

    /// Get the scope of this capability.
    ///
    /// The scope identifies the specific resource this capability authorizes
    /// access to. For channel capabilities, this is typically the channel name.
    /// For server-wide capabilities, this is `()`.
    #[inline]
    pub fn scope(&self) -> &T::Scope {
        &self.scope
    }

    /// Consume the capability and return the scope.
    ///
    /// This is useful when the capability is used exactly once and the
    /// scope value is needed for further processing.
    #[inline]
    pub fn into_scope(self) -> T::Scope {
        self.scope
    }
}

impl<T: Capability> fmt::Debug for Cap<T>
where
    T::Scope: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cap")
            .field("capability", &T::NAME)
            .field("scope", &self.scope)
            .finish()
    }
}

impl<T: Capability> fmt::Display for Cap<T>
where
    T::Scope: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cap<{}>({})", T::NAME, self.scope)
    }
}

/// Trait for capability types.
///
/// Each capability type defines:
/// - `Scope`: The type of resource it's scoped to (e.g., channel name, `()` for global)
/// - `NAME`: A human-readable name for logging and debugging
///
/// # Example
///
/// ```ignore
/// pub struct KickCap;
///
/// impl Capability for KickCap {
///     type Scope = String;  // Channel name
///     const NAME: &'static str = "channel:kick";
/// }
/// ```
pub trait Capability: 'static + Send + Sync {
    /// The type of resource this capability is scoped to.
    ///
    /// Common scope types:
    /// - `String` - Channel name for channel-level capabilities
    /// - `()` - Unit for server-wide capabilities (opers)
    type Scope: Clone + Send + Sync;

    /// Human-readable name of this capability (for logging).
    const NAME: &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test capability for unit tests
    struct TestCap;
    impl Capability for TestCap {
        type Scope = String;
        const NAME: &'static str = "test:cap";
    }

    struct GlobalTestCap;
    impl Capability for GlobalTestCap {
        type Scope = ();
        const NAME: &'static str = "test:global";
    }

    #[test]
    fn cap_has_correct_scope() {
        let cap = Cap::<TestCap>::new("test".to_string());
        assert_eq!(cap.scope(), "test");
    }

    #[test]
    fn cap_into_scope_consumes() {
        let cap = Cap::<TestCap>::new("channel".to_string());
        let scope = cap.into_scope();
        assert_eq!(scope, "channel");
        // cap is now moved, can't use it
    }

    #[test]
    fn cap_debug_format() {
        let cap = Cap::<TestCap>::new("debug".to_string());
        let debug = format!("{:?}", cap);
        assert!(debug.contains("test:cap"));
        assert!(debug.contains("debug"));
    }

    #[test]
    fn cap_display_format() {
        let cap = Cap::<TestCap>::new("display".to_string());
        let display = format!("{}", cap);
        assert_eq!(display, "Cap<test:cap>(display)");
    }

    #[test]
    fn global_cap_has_unit_scope() {
        let cap = Cap::<GlobalTestCap>::new(());
        assert_eq!(cap.scope(), &());
    }
}
