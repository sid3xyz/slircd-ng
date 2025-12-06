//! Response middleware for routing handler responses.
//!
//! Supports both direct forwarding to connection sender and capturing
//! for labeled-response batching.

use slirc_proto::Message;
use tokio::sync::{Mutex, mpsc};

/// Middleware for routing handler responses.
/// Direct forwards to the connection sender; Capturing buffers for labeled-response batching.
#[derive(Clone)]
pub enum ResponseMiddleware<'a> {
    Direct(&'a mpsc::Sender<Message>),
    Capturing(&'a Mutex<Vec<Message>>),
}

impl<'a> ResponseMiddleware<'a> {
    /// Send or buffer a message depending on middleware mode.
    pub async fn send(&self, msg: Message) -> Result<(), mpsc::error::SendError<Message>> {
        match self {
            Self::Direct(tx) => tx.send(msg).await,
            Self::Capturing(buf) => {
                let mut guard = buf.lock().await;
                guard.push(msg);
                Ok(())
            }
        }
    }
}
