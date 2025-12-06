//! Command handler registry and dispatch.
//!
//! The `Registry` manages command handlers and provides command usage statistics.
//! Includes IRC-aware instrumentation for observability (Innovation 3).

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
pub struct Registry {
    handlers: HashMap<&'static str, Box<dyn Handler>>,
    /// Command usage counters for STATS m
    command_counts: HashMap<&'static str, Arc<AtomicU64>>,
}

impl Registry {
    /// Create a new registry with all handlers registered.
    ///
    /// `webirc_blocks` is passed from config for WEBIRC authorization.
    pub fn new(webirc_blocks: Vec<crate::config::WebircBlock>) -> Self {
        let mut handlers: HashMap<&'static str, Box<dyn Handler>> = HashMap::new();

        // WEBIRC must be first to process before NICK/USER
        handlers.insert("WEBIRC", Box::new(WebircHandler::new(webirc_blocks)));

        // Connection/registration handlers
        handlers.insert("NICK", Box::new(NickHandler));
        handlers.insert("USER", Box::new(UserHandler));
        handlers.insert("PASS", Box::new(PassHandler));
        handlers.insert("PING", Box::new(PingHandler));
        handlers.insert("PONG", Box::new(PongHandler));
        handlers.insert("QUIT", Box::new(QuitHandler));
        handlers.insert("CAP", Box::new(CapHandler));
        handlers.insert("AUTHENTICATE", Box::new(AuthenticateHandler));
        handlers.insert("REGISTER", Box::new(RegisterHandler));

        // Channel handlers
        handlers.insert("JOIN", Box::new(JoinHandler));
        handlers.insert("PART", Box::new(PartHandler));
        handlers.insert("CYCLE", Box::new(CycleHandler));
        handlers.insert("TOPIC", Box::new(TopicHandler));
        handlers.insert("NAMES", Box::new(NamesHandler));
        handlers.insert("MODE", Box::new(ModeHandler));
        handlers.insert("KICK", Box::new(KickHandler));
        handlers.insert("LIST", Box::new(ListHandler));
        handlers.insert("INVITE", Box::new(InviteHandler));

        // Messaging handlers
        handlers.insert("PRIVMSG", Box::new(PrivmsgHandler));
        handlers.insert("NOTICE", Box::new(NoticeHandler));
        handlers.insert("TAGMSG", Box::new(TagmsgHandler));

        // User query handlers
        handlers.insert("WHO", Box::new(WhoHandler));
        handlers.insert("WHOIS", Box::new(WhoisHandler));
        handlers.insert("WHOWAS", Box::new(WhowasHandler));

        // Server query handlers
        handlers.insert("VERSION", Box::new(VersionHandler));
        handlers.insert("TIME", Box::new(TimeHandler));
        handlers.insert("ADMIN", Box::new(AdminHandler));
        handlers.insert("INFO", Box::new(InfoHandler));
        handlers.insert("LUSERS", Box::new(LusersHandler));
        handlers.insert("STATS", Box::new(StatsHandler));
        handlers.insert("MOTD", Box::new(MotdHandler));
        handlers.insert("MAP", Box::new(MapHandler));
        handlers.insert("RULES", Box::new(RulesHandler));
        handlers.insert("USERIP", Box::new(UseripHandler));
        handlers.insert("LINKS", Box::new(LinksHandler));
        handlers.insert("HELP", Box::new(HelpHandler));

        // Service query handlers (RFC 2812 §3.5)
        handlers.insert("SERVICE", Box::new(ServiceHandler));
        handlers.insert("SERVLIST", Box::new(ServlistHandler));
        handlers.insert("SQUERY", Box::new(SqueryHandler));

        // Misc handlers
        handlers.insert("AWAY", Box::new(AwayHandler));
        handlers.insert("USERHOST", Box::new(UserhostHandler));
        handlers.insert("ISON", Box::new(IsonHandler));
        handlers.insert("KNOCK", Box::new(KnockHandler));
        handlers.insert("SETNAME", Box::new(SetnameHandler));
        handlers.insert("SILENCE", Box::new(SilenceHandler));
        handlers.insert("MONITOR", Box::new(MonitorHandler));
        handlers.insert("CHATHISTORY", Box::new(ChatHistoryHandler));

        // Batch handler for IRCv3 message batching (draft/multiline)
        handlers.insert("BATCH", Box::new(BatchHandler));

        // Service aliases
        handlers.insert("NICKSERV", Box::new(NsHandler));
        handlers.insert("NS", Box::new(NsHandler)); // Shortcut for NickServ
        handlers.insert("CHANSERV", Box::new(CsHandler));
        handlers.insert("CS", Box::new(CsHandler)); // Shortcut for ChanServ

        // Operator handlers
        handlers.insert("OPER", Box::new(OperHandler));
        handlers.insert("KILL", Box::new(KillHandler));
        handlers.insert("WALLOPS", Box::new(WallopsHandler));
        handlers.insert("DIE", Box::new(DieHandler));
        handlers.insert("REHASH", Box::new(RehashHandler));
        handlers.insert("RESTART", Box::new(RestartHandler));
        handlers.insert("CHGHOST", Box::new(ChghostHandler));
        handlers.insert("VHOST", Box::new(VhostHandler));
        handlers.insert("TRACE", Box::new(TraceHandler));

        // Ban handlers
        handlers.insert("KLINE", Box::new(KlineHandler::kline()));
        handlers.insert("DLINE", Box::new(DlineHandler::dline()));
        handlers.insert("GLINE", Box::new(GlineHandler::gline()));
        handlers.insert("ZLINE", Box::new(ZlineHandler::zline()));
        handlers.insert("RLINE", Box::new(RlineHandler::rline()));
        handlers.insert("SHUN", Box::new(ShunHandler));
        handlers.insert("UNKLINE", Box::new(UnklineHandler::unkline()));
        handlers.insert("UNDLINE", Box::new(UndlineHandler::undline()));
        handlers.insert("UNGLINE", Box::new(UnglineHandler::ungline()));
        handlers.insert("UNZLINE", Box::new(UnzlineHandler::unzline()));
        handlers.insert("UNRLINE", Box::new(UnrlineHandler::unrline()));
        handlers.insert("UNSHUN", Box::new(UnshunHandler));

        // Admin SA* handlers
        handlers.insert("SAJOIN", Box::new(SajoinHandler));
        handlers.insert("SAPART", Box::new(SapartHandler));
        handlers.insert("SANICK", Box::new(SanickHandler));
        handlers.insert("SAMODE", Box::new(SamodeHandler));

        // Initialize command counters for all registered commands
        let mut command_counts = HashMap::new();
        for &cmd in handlers.keys() {
            command_counts.insert(cmd, Arc::new(AtomicU64::new(0)));
        }

        Self {
            handlers,
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
    pub async fn dispatch(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let cmd_name = msg.command_name().to_ascii_uppercase();

        if let Some(handler) = self.handlers.get(cmd_name.as_str()) {
            // Increment command counter (counters are created for all handlers in new())
            // We use expect() here because the invariant is that all handlers have counters.
            // If this fails, it indicates a logic error in Registry::new().
            let counter = self
                .command_counts
                .get(cmd_name.as_str())
                .expect("Command counter missing for registered handler");
            counter.fetch_add(1, Ordering::Relaxed);

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
            // Send ERR_UNKNOWNCOMMAND for unrecognized commands
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
