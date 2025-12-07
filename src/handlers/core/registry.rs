//! Command handler registry and dispatch.
//!
//! The `Registry` manages command handlers and provides command usage statistics.
//! Includes IRC-aware instrumentation for observability (Innovation 3).
//!
//! ## Typestate Dispatch (Innovation 1)
//!
//! Handlers are stored in phase-specific maps based on registration requirements:
//! - **`pre_reg_handlers`**: NICK, USER, PASS, CAP, etc. (valid before registration)
//! - **`post_reg_handlers`**: PRIVMSG, JOIN, etc. (require registration)
//! - **`universal_handlers`**: QUIT, PING, PONG (valid in any state)
//!
//! The registry dispatches based on connection state using the type system.
//! Post-registration handlers are *not in the map* for unregistered connections,
//! making invalid dispatch a structural impossibility.

use super::context::{Context, Handler, HandlerResult};
use crate::handlers::{
    account::RegisterHandler,
    admin::{SajoinHandler, SamodeHandler, SanickHandler, SapartHandler},
    bans::{
        DlineHandler, GlineHandler, KlineHandler, RlineHandler, ShunHandler, UndlineHandler,
        UnglineHandler, UnklineHandler, UnrlineHandler, UnshunHandler, UnzlineHandler,
        ZlineHandler,
    },
    batch::BatchHandler,
    cap::{AuthenticateHandler, CapHandler},
    channel::{
        CycleHandler, InviteHandler, JoinHandler, KickHandler, KnockHandler, ListHandler,
        NamesHandler, PartHandler, TopicHandler,
    },
    chathistory::ChatHistoryHandler,
    connection::{
        NickHandler, PassHandler, PingHandler, PongHandler, QuitHandler, UserHandler, WebircHandler,
    },
    helpers::{err_unknowncommand, with_label},
    messaging::{NoticeHandler, PrivmsgHandler, TagmsgHandler},
    mode::ModeHandler,
    monitor::MonitorHandler,
    oper::{
        ChghostHandler, DieHandler, KillHandler, OperHandler, RehashHandler, RestartHandler,
        TraceHandler, VhostHandler, WallopsHandler,
    },
    server_query::{
        AdminHandler, HelpHandler, InfoHandler, LinksHandler, LusersHandler, MapHandler,
        MotdHandler, RulesHandler, ServiceHandler, ServlistHandler, SqueryHandler, StatsHandler,
        TimeHandler, UseripHandler, VersionHandler,
    },
    service_aliases::{CsHandler, NsHandler},
    user_query::{IsonHandler, UserhostHandler, WhoHandler, WhoisHandler, WhowasHandler},
    user_status::{AwayHandler, SetnameHandler, SilenceHandler},
};
use crate::telemetry::CommandTimer;
use slirc_proto::MessageRef;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{Instrument, debug, span, Level};

/// Registry of command handlers.
///
/// Handlers are organized into three maps by registration phase (Innovation 1):
/// - `pre_reg_handlers`: Commands valid before registration
/// - `post_reg_handlers`: Commands requiring registration
/// - `universal_handlers`: Commands valid in any state
pub struct Registry {
    /// Handlers for pre-registration commands (NICK, USER, PASS, CAP, etc.)
    pre_reg_handlers: HashMap<&'static str, Box<dyn Handler>>,
    /// Handlers for post-registration commands (PRIVMSG, JOIN, etc.)
    post_reg_handlers: HashMap<&'static str, Box<dyn Handler>>,
    /// Handlers valid in any state (QUIT, PING, PONG)
    universal_handlers: HashMap<&'static str, Box<dyn Handler>>,
    /// Command usage counters for STATS m
    command_counts: HashMap<&'static str, Arc<AtomicU64>>,
}

impl Registry {
    /// Create a new registry with all handlers registered.
    ///
    /// Handlers are placed into the appropriate phase map based on IRC protocol rules.
    /// `webirc_blocks` is passed from config for WEBIRC authorization.
    pub fn new(webirc_blocks: Vec<crate::config::WebircBlock>) -> Self {
        let mut pre_reg_handlers: HashMap<&'static str, Box<dyn Handler>> = HashMap::new();
        let mut post_reg_handlers: HashMap<&'static str, Box<dyn Handler>> = HashMap::new();
        let mut universal_handlers: HashMap<&'static str, Box<dyn Handler>> = HashMap::new();

        // ====================================================================
        // Universal handlers (valid in any state)
        // ====================================================================
        universal_handlers.insert("QUIT", Box::new(QuitHandler));
        universal_handlers.insert("PING", Box::new(PingHandler));
        universal_handlers.insert("PONG", Box::new(PongHandler));

        // ====================================================================
        // Pre-registration handlers (valid before registration completes)
        // ====================================================================
        // WEBIRC must be first to process before NICK/USER
        pre_reg_handlers.insert("WEBIRC", Box::new(WebircHandler::new(webirc_blocks)));
        pre_reg_handlers.insert("NICK", Box::new(NickHandler));
        pre_reg_handlers.insert("USER", Box::new(UserHandler));
        pre_reg_handlers.insert("PASS", Box::new(PassHandler));
        pre_reg_handlers.insert("CAP", Box::new(CapHandler));
        pre_reg_handlers.insert("AUTHENTICATE", Box::new(AuthenticateHandler));
        pre_reg_handlers.insert("REGISTER", Box::new(RegisterHandler));

        // ====================================================================
        // Post-registration handlers (require completed registration)
        // ====================================================================

        // Channel handlers
        post_reg_handlers.insert("JOIN", Box::new(JoinHandler));
        post_reg_handlers.insert("PART", Box::new(PartHandler));
        post_reg_handlers.insert("CYCLE", Box::new(CycleHandler));
        post_reg_handlers.insert("TOPIC", Box::new(TopicHandler));
        post_reg_handlers.insert("NAMES", Box::new(NamesHandler));
        post_reg_handlers.insert("MODE", Box::new(ModeHandler));
        post_reg_handlers.insert("KICK", Box::new(KickHandler));
        post_reg_handlers.insert("LIST", Box::new(ListHandler));
        post_reg_handlers.insert("INVITE", Box::new(InviteHandler));

        // Messaging handlers
        post_reg_handlers.insert("PRIVMSG", Box::new(PrivmsgHandler));
        post_reg_handlers.insert("NOTICE", Box::new(NoticeHandler));
        post_reg_handlers.insert("TAGMSG", Box::new(TagmsgHandler));

        // User query handlers
        post_reg_handlers.insert("WHO", Box::new(WhoHandler));
        post_reg_handlers.insert("WHOIS", Box::new(WhoisHandler));
        post_reg_handlers.insert("WHOWAS", Box::new(WhowasHandler));

        // Server query handlers
        post_reg_handlers.insert("VERSION", Box::new(VersionHandler));
        post_reg_handlers.insert("TIME", Box::new(TimeHandler));
        post_reg_handlers.insert("ADMIN", Box::new(AdminHandler));
        post_reg_handlers.insert("INFO", Box::new(InfoHandler));
        post_reg_handlers.insert("LUSERS", Box::new(LusersHandler));
        post_reg_handlers.insert("STATS", Box::new(StatsHandler));
        post_reg_handlers.insert("MOTD", Box::new(MotdHandler));
        post_reg_handlers.insert("MAP", Box::new(MapHandler));
        post_reg_handlers.insert("RULES", Box::new(RulesHandler));
        post_reg_handlers.insert("USERIP", Box::new(UseripHandler));
        post_reg_handlers.insert("LINKS", Box::new(LinksHandler));
        post_reg_handlers.insert("HELP", Box::new(HelpHandler));

        // Service query handlers (RFC 2812 §3.5)
        post_reg_handlers.insert("SERVICE", Box::new(ServiceHandler));
        post_reg_handlers.insert("SERVLIST", Box::new(ServlistHandler));
        post_reg_handlers.insert("SQUERY", Box::new(SqueryHandler));

        // Misc handlers
        post_reg_handlers.insert("AWAY", Box::new(AwayHandler));
        post_reg_handlers.insert("USERHOST", Box::new(UserhostHandler));
        post_reg_handlers.insert("ISON", Box::new(IsonHandler));
        post_reg_handlers.insert("KNOCK", Box::new(KnockHandler));
        post_reg_handlers.insert("SETNAME", Box::new(SetnameHandler));
        post_reg_handlers.insert("SILENCE", Box::new(SilenceHandler));
        post_reg_handlers.insert("MONITOR", Box::new(MonitorHandler));
        post_reg_handlers.insert("CHATHISTORY", Box::new(ChatHistoryHandler));

        // Batch handler for IRCv3 message batching (draft/multiline)
        post_reg_handlers.insert("BATCH", Box::new(BatchHandler));

        // Service aliases
        post_reg_handlers.insert("NICKSERV", Box::new(NsHandler));
        post_reg_handlers.insert("NS", Box::new(NsHandler)); // Shortcut for NickServ
        post_reg_handlers.insert("CHANSERV", Box::new(CsHandler));
        post_reg_handlers.insert("CS", Box::new(CsHandler)); // Shortcut for ChanServ

        // Operator handlers
        post_reg_handlers.insert("OPER", Box::new(OperHandler));
        post_reg_handlers.insert("KILL", Box::new(KillHandler));
        post_reg_handlers.insert("WALLOPS", Box::new(WallopsHandler));
        post_reg_handlers.insert("DIE", Box::new(DieHandler));
        post_reg_handlers.insert("REHASH", Box::new(RehashHandler));
        post_reg_handlers.insert("RESTART", Box::new(RestartHandler));
        post_reg_handlers.insert("CHGHOST", Box::new(ChghostHandler));
        post_reg_handlers.insert("VHOST", Box::new(VhostHandler));
        post_reg_handlers.insert("TRACE", Box::new(TraceHandler));

        // Ban handlers
        post_reg_handlers.insert("KLINE", Box::new(KlineHandler::kline()));
        post_reg_handlers.insert("DLINE", Box::new(DlineHandler::dline()));
        post_reg_handlers.insert("GLINE", Box::new(GlineHandler::gline()));
        post_reg_handlers.insert("ZLINE", Box::new(ZlineHandler::zline()));
        post_reg_handlers.insert("RLINE", Box::new(RlineHandler::rline()));
        post_reg_handlers.insert("SHUN", Box::new(ShunHandler));
        post_reg_handlers.insert("UNKLINE", Box::new(UnklineHandler::unkline()));
        post_reg_handlers.insert("UNDLINE", Box::new(UndlineHandler::undline()));
        post_reg_handlers.insert("UNGLINE", Box::new(UnglineHandler::ungline()));
        post_reg_handlers.insert("UNZLINE", Box::new(UnzlineHandler::unzline()));
        post_reg_handlers.insert("UNRLINE", Box::new(UnrlineHandler::unrline()));
        post_reg_handlers.insert("UNSHUN", Box::new(UnshunHandler));

        // Admin SA* handlers
        post_reg_handlers.insert("SAJOIN", Box::new(SajoinHandler));
        post_reg_handlers.insert("SAPART", Box::new(SapartHandler));
        post_reg_handlers.insert("SANICK", Box::new(SanickHandler));
        post_reg_handlers.insert("SAMODE", Box::new(SamodeHandler));

        // Initialize command counters for all registered commands
        let mut command_counts = HashMap::new();
        for &cmd in pre_reg_handlers.keys() {
            command_counts.insert(cmd, Arc::new(AtomicU64::new(0)));
        }
        for &cmd in post_reg_handlers.keys() {
            command_counts.insert(cmd, Arc::new(AtomicU64::new(0)));
        }
        for &cmd in universal_handlers.keys() {
            command_counts.insert(cmd, Arc::new(AtomicU64::new(0)));
        }

        Self {
            pre_reg_handlers,
            post_reg_handlers,
            universal_handlers,
            command_counts,
        }
    }

    /// Get command usage statistics for STATS m.
    pub fn get_command_stats(&self) -> Vec<(&'static str, u64)> {
        let mut stats: Vec<_> = self
            .command_counts
            .iter()
            .map(|(cmd, count)| (*cmd, count.load(Ordering::Relaxed)))
            .filter(|(_, count)| *count > 0) // Only include used commands
            .collect();

        // Sort by usage count (descending)
        stats.sort_by(|a, b| b.1.cmp(&a.1));
        stats
    }

    /// Dispatch a message to the appropriate handler.
    ///
    /// Uses `msg.command_name()` to get the command name directly from the
    /// zero-copy `MessageRef`. Includes IRC-aware instrumentation for tracing
    /// and metrics (Innovation 3).
    ///
    /// ## Typestate Dispatch (Innovation 1)
    ///
    /// Handler lookup is based on connection state:
    /// - **Universal handlers**: Always checked first (QUIT, PING, PONG)
    /// - **Pre-reg handlers**: Checked for unregistered connections
    /// - **Post-reg handlers**: Only accessible to registered connections
    ///
    /// Post-registration handlers are structurally unavailable to unregistered
    /// connections - they are simply not in the lookup path.
    pub async fn dispatch(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let cmd_name = msg.command_name().to_ascii_uppercase();

        // Typestate dispatch: Look up handler in phase-appropriate map
        // Universal handlers are always available, then check phase-specific map
        let handler: Option<&Box<dyn Handler>> = self
            .universal_handlers
            .get(cmd_name.as_str())
            .or_else(|| {
                if ctx.handshake.registered {
                    // Registered: check post-reg handlers, then pre-reg (for NICK changes etc.)
                    self.post_reg_handlers
                        .get(cmd_name.as_str())
                        .or_else(|| self.pre_reg_handlers.get(cmd_name.as_str()))
                } else {
                    // Unregistered: only pre-reg handlers available
                    // Post-reg handlers are structurally inaccessible!
                    self.pre_reg_handlers.get(cmd_name.as_str())
                }
            });

        if let Some(handler) = handler {
            // Increment command counter
            if let Some(counter) = self.command_counts.get(cmd_name.as_str()) {
                counter.fetch_add(1, Ordering::Relaxed);
            }

            // Extract IRC context for tracing
            let source_nick = ctx.handshake.nick.as_deref();
            let channel = msg.arg(0).filter(|a| a.starts_with('#') || a.starts_with('&'));
            let msgid = crate::telemetry::extract_msgid(msg);

            // Create instrumented span
            let irc_span = span!(
                Level::DEBUG,
                "irc.command",
                command = %cmd_name,
                uid = %ctx.uid,
                source_nick = source_nick,
                channel = channel,
                msgid = msgid.as_deref(),
                remote_addr = %ctx.remote_addr,
            );

            // Start timing for metrics
            let _timer = CommandTimer::new(&cmd_name);

            // Execute handler within the span
            let result = handler.handle(ctx, msg).instrument(irc_span).await;

            // Record errors for metrics
            if let Err(ref e) = result {
                let error_kind = match e {
                    super::context::HandlerError::NeedMoreParams => "need_more_params",
                    super::context::HandlerError::NoTextToSend => "no_text_to_send",
                    super::context::HandlerError::NicknameInUse(_) => "nickname_in_use",
                    super::context::HandlerError::ErroneousNickname(_) => "erroneous_nickname",
                    super::context::HandlerError::NotRegistered => "not_registered",
                    super::context::HandlerError::AccessDenied => "access_denied",
                    super::context::HandlerError::AlreadyRegistered => "already_registered",
                    super::context::HandlerError::NickOrUserMissing => "nick_or_user_missing",
                    super::context::HandlerError::Send(_) => "send_error",
                    super::context::HandlerError::Quit(_) => "quit",
                    super::context::HandlerError::Internal(_) => "internal_error",
                };
                crate::metrics::record_command_error(&cmd_name, error_kind);
                debug!(command = %cmd_name, error = %e, "Command error");
            }

            result
        } else {
            // Handler not found in accessible maps
            // For unregistered clients trying post-reg commands, return ERR_NOTREGISTERED
            if !ctx.handshake.registered && self.post_reg_handlers.contains_key(cmd_name.as_str()) {
                debug!(
                    command = %cmd_name,
                    uid = %ctx.uid,
                    "Command rejected: not registered (typestate)"
                );
                crate::metrics::record_command_error(&cmd_name, "not_registered");
                return Err(super::context::HandlerError::NotRegistered);
            }

            // Otherwise, unknown command
            use super::context::get_nick_or_star;
            let nick = get_nick_or_star(ctx).await;
            let reply = err_unknowncommand(&ctx.matrix.server_info.name, &nick, &cmd_name);
            // Attach label for labeled-response capability
            let reply = with_label(reply, ctx.label.as_deref());
            ctx.sender.send(reply).await?;

            // Record unknown command metric
            crate::metrics::record_command_error(&cmd_name, "unknown_command");

            Ok(())
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}
