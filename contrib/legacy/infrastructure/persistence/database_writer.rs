//! DatabaseWriter: Buffers and batches database writes for performance.
//!
//! Gonzo admin: This is the high-octane, caffeine-fueled write buffer for the SLIRCD database. It takes your precious little inserts and lines them up for a full-throttle, batched commit. No more one-at-a-time slowpoke writes. This is the express lane.

use crate::infrastructure::persistence::database::Database;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::JoinHandle;

/// Represents a database write operation.
pub enum DbWrite {
    InsertUserHistory(crate::infrastructure::persistence::database::UserQuitRecord<'static>),
    // Add more variants for other write types as needed
}

/// Buffers and batches database writes, committing them in bulk for performance.
pub struct DatabaseWriter {
    sender: Sender<DbWrite>,
    _handle: JoinHandle<()>,
}

impl DatabaseWriter {
    pub fn new(db: Arc<Database>) -> Self {
        let (sender, mut receiver): (Sender<DbWrite>, Receiver<DbWrite>) = mpsc::channel(1000);
        let db = db.clone();
        let handle = tokio::spawn(async move {
            let mut buffer = Vec::with_capacity(100);
            loop {
                tokio::select! {
                    Some(write) = receiver.recv() => {
                        buffer.push(write);
                        if buffer.len() >= 100 {
                            Self::flush(&db, &mut buffer).await;
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(1000)), if !buffer.is_empty() => {
                        Self::flush(&db, &mut buffer).await;
                    }
                }
            }
        });
        Self {
            sender,
            _handle: handle,
        }
    }

    pub async fn write(&self, op: DbWrite) {
        let _ = self.sender.send(op).await;
    }

    async fn flush(db: &Database, buffer: &mut Vec<DbWrite>) {
        for op in buffer.drain(..) {
            match op {
                DbWrite::InsertUserHistory(record) => {
                    let _ = db.record_user_quit(record).await;
                } // Add more variants as needed
            }
        }
    }
}
