//! Message dispatch pipeline for the event loop.
//!
//! Extracts the message processing logic from the main event loop into
//! a dedicated module for better testability and maintainability.

use super::context::ConnectionContext;
use crate::handlers::{Context, HandlerResult, ResponseMiddleware, process_batch_message};
use crate::state::RegisteredState;
use slirc_proto::message::MessageRef;
use slirc_proto::Message;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, warn};

/// Result of processing a single message through the pipeline.
pub enum DispatchResult {
    /// Continue the event loop normally
    Continue,
    /// Client issued QUIT - break from event loop with optional message
    Quit(Option<String>),
    /// Write error occurred - break from event loop
    WriteError,
}

/// Parameters for processing a single message.
pub struct ProcessParams<'a> {
    pub msg: &'a Message,
    pub label: Option<String>,
    pub uid: &'a str,
    pub addr: SocketAddr,
    pub reg_state: &'a mut RegisteredState,
}

/// Process a single incoming message through the full pipeline.
///
/// Pipeline stages:
/// 1. Update last_active timestamp for IDLE tracking
/// 2. Batch message processing (if applicable)
/// 3. Select response middleware (capturing for labeled-response)
/// 4. Dispatch to handler registry
/// 5. Handle errors and QUIT
/// 6. Send labeled-response if applicable
///
/// Returns a `DispatchResult` indicating whether to continue or break the loop.
pub async fn process_message<'a>(
    conn: &mut ConnectionContext<'a>,
    params: ProcessParams<'_>,
    outgoing_tx: &mpsc::Sender<Arc<Message>>,
    outgoing_rx: &mut mpsc::Receiver<Arc<Message>>,
) -> DispatchResult {
    let ProcessParams {
        msg,
        label,
        uid,
        addr,
        reg_state,
    } = params;

    // Stage 1: Update last active timestamp
    conn.matrix.user_manager.update_last_active(uid).await;
    debug!(raw = ?msg, "Received message");

    // Stage 2: Batch processing
    let raw_str = msg.to_string();
    let batch_result = if let Ok(msg_ref) = MessageRef::parse(&raw_str) {
        process_batch_message(reg_state, &msg_ref, &conn.matrix.server_info.name)
    } else {
        Ok(None)
    };

    match batch_result {
        Ok(Some(_batch_ref)) => {
            debug!("Message absorbed into active batch");
            return DispatchResult::Continue;
        }
        Ok(None) => {}
        Err(fail_msg) => {
            warn!(error = %fail_msg, "Batch processing error");
            reg_state.active_batch = None;
            reg_state.active_batch_ref = None;
            if let Ok(fail) = fail_msg.parse::<Message>()
                && outgoing_tx.send(Arc::new(fail)).await.is_err()
            {
                return DispatchResult::Continue;
            }
            return DispatchResult::Continue;
        }
    }

    // Stage 3: Select middleware for labeled-response
    let capture_buffer: Option<Mutex<Vec<Message>>> =
        label.as_ref().map(|_| Mutex::new(Vec::new()));
    let sender_middleware = if let Some(buf) = capture_buffer.as_ref() {
        ResponseMiddleware::Capturing(buf)
    } else {
        ResponseMiddleware::Direct(outgoing_tx)
    };

    // Stage 4: Dispatch to handler
    let (dispatch_result, suppress_ack) = dispatch_to_handler(
        conn,
        &raw_str,
        uid,
        &addr,
        reg_state,
        label.clone(),
        sender_middleware,
    )
    .await;

    // Stage 5: Handle errors
    if let Err(e) = dispatch_result {
        debug!(error = ?e, "Handler error");

        if let crate::handlers::HandlerError::Quit(quit_msg) = e {
            // Drain pending outgoing messages before quitting
            while let Ok(msg) = outgoing_rx.try_recv() {
                if conn.transport.write_message(&msg).await.is_err() {
                    return DispatchResult::WriteError;
                }
            }

            let error_reply = super::helpers::closing_link_error(&addr, quit_msg.as_deref());
            if conn.transport.write_message(&error_reply).await.is_err() {
                return DispatchResult::WriteError;
            }
            return DispatchResult::Quit(quit_msg);
        } else {
            // Other errors - send error reply
            let nick = &reg_state.nick;
            if let Some(reply) =
                super::error_handling::handler_error_to_reply_owned(&conn.matrix.server_info.name, nick, &e, msg)
                && conn.transport.write_message(&reply).await.is_err()
            {
                return DispatchResult::WriteError;
            }
        }
    }

    // Stage 6: Labeled-response handling
    if let Some(label_str) = label
        && let Some(buf) = capture_buffer
    {
        let mut messages = buf.lock().await;
        super::event_loop::send_labeled_response(
            conn.transport,
            &conn.matrix.server_info.name,
            &label_str,
            &mut messages,
            suppress_ack,
        )
        .await;
    }

    DispatchResult::Continue
}

/// Dispatch a message to the handler registry.
async fn dispatch_to_handler<'a>(
    conn: &ConnectionContext<'a>,
    raw_str: &str,
    uid: &str,
    addr: &SocketAddr,
    reg_state: &mut RegisteredState,
    label: Option<String>,
    sender: ResponseMiddleware<'_>,
) -> (HandlerResult, bool) {
    if let Ok(msg_ref) = MessageRef::parse(raw_str) {
        let mut ctx = Context {
            uid,
            matrix: conn.matrix,
            sender,
            state: reg_state,
            db: conn.db,
            remote_addr: *addr,
            label,
            suppress_labeled_ack: false,
            active_batch_id: None,
            registry: conn.registry,
        };

        let result = conn.registry.dispatch_post_reg(&mut ctx, &msg_ref).await;
        (result, ctx.suppress_labeled_ack)
    } else {
        // Should not happen, but handle gracefully
        (Ok(()), false)
    }
}
