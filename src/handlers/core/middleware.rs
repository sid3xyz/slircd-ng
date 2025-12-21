//! Response middleware for routing handler responses.
//!
//! Supports both direct forwarding to connection sender and capturing
//! for labeled-response batching.
//!
//! # SendQ Overflow Protection
//!
//! When the outgoing message queue is full (slow consumer), messages are
//! dropped and the error is propagated to trigger client disconnection.
//! This prevents memory exhaustion from clients that don't read their data.

use slirc_proto::Message;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};

/// Timeout for attempting to send to a slow consumer before giving up.
/// 5 seconds is generous - a healthy client should never hit this.
const SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// Middleware for routing handler responses.
/// Direct forwards to the connection sender; Capturing buffers for labeled-response batching.
#[derive(Clone)]
pub enum ResponseMiddleware<'a> {
    Direct(&'a mpsc::Sender<Message>),
    Capturing(&'a Mutex<Vec<Message>>),
}

impl<'a> ResponseMiddleware<'a> {
    /// Send or buffer a message depending on middleware mode.
    ///
    /// For Direct mode, uses a timeout to detect slow consumers. If the
    /// send times out, the message is dropped and an error is returned
    /// to signal that the client should be disconnected.
    pub async fn send(&self, msg: Message) -> Result<(), mpsc::error::SendError<Message>> {
        match self {
            Self::Direct(tx) => {
                // Use timeout to detect slow consumers and avoid blocking indefinitely
                match tokio::time::timeout(SEND_TIMEOUT, tx.send(msg.clone())).await {
                    Ok(result) => result,
                    Err(_timeout) => {
                        // Slow consumer - SendQ overflow
                        tracing::warn!(
                            "SendQ overflow: client not reading (timeout after {:?})",
                            SEND_TIMEOUT
                        );
                        Err(mpsc::error::SendError(msg))
                    }
                }
            }
            Self::Capturing(buf) => {
                let mut guard = buf.lock().await;
                guard.push(msg);
                Ok(())
            }
        }
    }
}
