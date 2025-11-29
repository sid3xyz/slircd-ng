Zero-Copy Architecture Migration PlanStatus: Approved & ExpandedTarget Architecture: Unified Event Loop (tokio::select!)Goal: Eliminate per-message allocation in the hot loop.Strict Requirement: LEAVE NO LEGACY CODE. All allocation-based patterns must be removed.1. Executive SummaryThis document outlines the refactoring steps required to transition slircd-ng from a "Low-Copy" actor model (separate Reader/Writer tasks communicating via channels) to a True Zero-Copy architecture.The Core Change: We will consolidate the connection logic into a single async task using tokio::select!. This allows the business logic (Handlers) to operate directly on the borrowed buffer provided by the ZeroCopyTransport, respecting Rust's ownership rules without clone() or .to_owned().Cleanliness Mandate:Delete the old Reader/Writer task spawning logic.Delete the internal mpsc channels used for passing Message objects between tasks.Delete any Handler implementation that takes &Message.Remove all to_owned() calls on MessageRef inside handlers.2. Architecture DiagramLegacy Architecture (DELETE THIS):[Reader Task] --(Message)--> [Actor Task] --(Message)--> [Writer Task]
      ^             ^            ^             ^            ^
      |             |            |             |            |
ZeroCopyTransport   |       Handler Logic      |       FramedWrite
(Allocates here)────┘                          └───────(Serializes)
Target Architecture (IMPLEMENT THIS):      ┌──────────────────────────────────────────────────┐
      │               Unified Connection Task            │
      │                                                  │
      │  ┌─────────────────┐       ┌──────────────────┐  │
      │  │ ZeroCopyReader  │       │   FramedWrite    │  │
      │  └────────┬────────┘       └────────▲─────────┘  │
      │           │ (Borrow)                │            │
      │           ▼                         │            │
      │    tokio::select! ◄─────────────────┼────────────┐
      │    │      │                         │            │
      │    │      ▼                         │            │
      │    │  [Handlers] ─────────▶ [Outgoing Queue]     │
      │    │  (Zero Alloc)                               │
      │    └─────────────────────────────────────────────┘
      └──────────────────────────────────────────────────┘
3. Implementation ChecklistPhase 1: Interface Updates (High Priority)Goal: Change the contract. This will break the build, forcing a complete refactor.[ ] Modify src/handlers/mod.rs:Update Handler trait signature to take MessageRef:async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult;
Update Registry::dispatch to use msg.command_name().Remove any fallback code that tries to handle owned Message types.Phase 2: Handler Refactoring (The Heavy Lift)Goal: Rewrite all handlers to use MessageRef API. Do NOT use to_owned(). Use msg.arg(n) to access arguments as &str.Mandatory Refactoring Pattern:// ❌ BAD (Legacy):
// let nick = match &msg.command { Command::NICK(n) => n.clone(), ... };

// ✅ GOOD (Zero-Copy):
// let nick = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
[ ] Refactor src/handlers/connection.rs:NickHandler, UserHandler, PingHandler, PongHandler, QuitHandler.[ ] Refactor src/handlers/channel.rs:JoinHandler: Handle comma-separated channels by splitting the borrowed &str.PartHandler, TopicHandler, NamesHandler, KickHandler.[ ] Refactor src/handlers/messaging.rs:PrivmsgHandler, NoticeHandler.[ ] Refactor src/handlers/mode.rs:ModeHandler: Use msg.args() slice (e.g. &msg.args()[1..]) to pass to Mode::as_channel_modes.[ ] Refactor Remaining Handlers:oper.rs, misc.rs, server_query.rs, user_query.rs, admin.rs, bans.rs.cap.rs: Parse subcommands manually: msg.arg(0).unwrap().parse::<CapSubCommand>().Phase 3: The Unified Core LoopGoal: Replace the Connection::run logic entirely.[ ] Create src/network/limit.rs:Implement RateLimiter struct.[ ] Rewrite src/network/connection.rs:Delete the entire existing run method body.Implement the tokio::select! loop (see Section 5.3).Verify that no tokio::spawn is used for Reader/Writer tasks.Initialize RateLimiter.Ensure outgoing_rx is drained in the same loop (to handle routing from other users).Phase 4: Cleanup & VerificationGoal: Ensure no technical debt remains.[ ] Remove Unused Imports: Run cargo fix or manually remove unused slirc_proto::Command imports (since we now use MessageRef).[ ] Verify No Allocations: Grep for to_owned() and clone() in src/handlers/. There should be NONE related to message parsing.[ ] Check Dependencies: Ensure slirc-proto dependency is up to date and supports the required zero-copy operations.5. Detailed Code Reference5.1. The New Handler Trait// src/handlers/mod.rs
use slirc_proto::MessageRef;

#[async_trait]
pub trait Handler: Send + Sync {
    // STRICTLY borrowed MessageRef. No owned Message.
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult;
}
5.2. The Rate Limiter// src/network/limit.rs
use std::time::Instant;

pub struct RateLimiter {
    tokens: f32,
    last_check: Instant,
    rate: f32,
    capacity: f32,
}

impl RateLimiter {
    pub fn new(rate: f32, capacity: f32) -> Self {
        Self { tokens: capacity, last_check: Instant::now(), rate, capacity }
    }

    pub fn check(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_check).as_secs_f32();
        self.last_check = now;
        self.tokens = (self.tokens + elapsed * self.rate).min(self.capacity);
        
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}
5.3. The Unified Loop (Connection::run)Instructions: Replace the existing run function COMPLETELY with this logic. Do not keep the old spawn logic commented out. Delete it.// src/network/connection.rs

// ... imports ...

pub async fn run(self) -> anyhow::Result<()> {
    // ... (Handshake logic remains unchanged for now - it happens before the hot loop) ...

    // --- PHASE 2: UNIFIED ZERO-COPY LOOP ---
    
    // 1. Split Transport
    let parts = transport.into_parts()?;
    let (read_parts, write_parts) = parts.split();

    // 2. Setup Components
    let mut reader = ZeroCopyTransport::with_buffer(read_parts.half, read_parts.read_buf);
    let mut writer = FramedWrite::new(write_parts.half, write_parts.codec);
    let mut rate_limit = crate::network::limit::RateLimiter::new(10.0, 20.0); // 10 msg/sec

    // 3. Setup Internal Routing Channel (Only for external messages)
    // We register this sender so OTHER users can send us messages (PRIVMSG etc).
    // We also use it for our own handlers to queue responses if needed.
    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<Message>(32);
    self.matrix.register_sender(&self.uid, outgoing_tx.clone());

    info!("Entering Unified Zero-Copy Loop");

    loop {
        tokio::select! {
            // BRANCH A: Network Input (Zero-Copy)
            // 'msg_ref' is borrowed from 'reader'. It exists ONLY inside this match block.
            result = reader.next() => {
                match result {
                    Some(Ok(msg_ref)) => {
                        // 1. Flood Protection
                        if !rate_limit.check() {
                            warn!("Rate limit exceeded for {}", self.uid);
                            let _ = writer.send(Message::from(Command::ERROR("Excess Flood".into()))).await;
                            break;
                        }

                        // 2. Dispatch
                        let mut ctx = Context {
                            uid: &self.uid,
                            matrix: &self.matrix,
                            sender: &outgoing_tx, 
                            handshake: &mut handshake,
                            db: &self.db,
                            remote_addr: self.addr,
                        };

                        if let Err(e) = self.registry.dispatch(&mut ctx, &msg_ref).await {
                            if matches!(e, HandlerError::NotRegistered) { break; }
                            debug!("Handler error: {}", e);
                        }
                    }
                    Some(Err(e)) => {
                        warn!("Read error: {}", e);
                        break;
                    }
                    None => {
                        info!("Client disconnected");
                        break;
                    }
                }
            }

            // BRANCH B: Outgoing Messages
            // Handles responses queued by handlers AND messages routed from other users
            Some(msg) = outgoing_rx.recv() => {
                if let Err(e) = writer.send(msg).await {
                    warn!("Write error: {}", e);
                    break;
                }
            }
        }
    }

    // ... (Cleanup logic) ...
    Ok(())
}
6. Final Code Quality GateBefore submitting changes, verify:Compilation: cargo check must pass without errors.No Warnings: cargo clippy should be clean.No Dead Code: Ensure Command::* enum variants are not being destructured in handlers (we only use msg.command_name() string now).No Allocations: Verify msg.args() (returning &[&str]) is used instead of msg.args.clone().