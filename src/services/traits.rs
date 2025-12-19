use crate::services::ServiceEffect;
use crate::state::Matrix;
use async_trait::async_trait;
use std::sync::Arc;

/// Trait for IRC services.
///
/// Services are virtual entities that handle commands directed at them.
/// They produce effects which are then applied to the server state.
#[async_trait]
pub trait Service: Send + Sync {
    /// Get the canonical name of the service (e.g., "NickServ").
    fn name(&self) -> &'static str;

    /// Get the aliases for this service (e.g., "NS").
    fn aliases(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Handle a message directed to this service.
    ///
    /// # Arguments
    /// * `matrix` - The server state.
    /// * `uid` - The UID of the sender.
    /// * `nick` - The nickname of the sender.
    /// * `text` - The message text (command and arguments).
    ///
    /// # Returns
    /// A list of effects to apply.
    async fn handle(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        text: &str,
    ) -> Vec<ServiceEffect>;
}
