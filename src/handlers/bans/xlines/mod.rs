//! X-line ban command handlers.
//!
//! Server-wide ban commands (operator-only):
//! - KLINE/UNKLINE: Ban/unban by nick!user@host mask
//! - DLINE/UNDLINE: Ban/unban by IP address
//! - GLINE/UNGLINE: Global ban/unban by nick!user@host mask
//! - ZLINE/UNZLINE: Global IP ban/unban (skips DNS)
//! - RLINE/UNRLINE: Ban/unban by realname (GECOS)
//!
//! Uses a trait-based generic handler system to minimize code duplication.

use super::common::{BanType, disconnect_matching_ban};
use crate::caps::CapabilityAuthority;
use crate::db::{Database, DbError};
use crate::handlers::{Context,
    HandlerResult, PostRegHandler, server_notice,
};
use crate::state::{Matrix, RegisteredState};
use async_trait::async_trait;
use ipnet::IpNet;
use slirc_proto::{MessageRef, Response};
use std::sync::Arc;

// -----------------------------------------------------------------------------
// BanConfig Trait
// -----------------------------------------------------------------------------

/// Configuration trait for X-line handlers.
///
/// Implementors define the specifics for each ban type (K, G, D, Z, R-line).
#[async_trait]
pub trait BanConfig: Send + Sync + 'static {
    /// Command name for error messages (e.g., "KLINE", "GLINE").
    fn command_name(&self) -> &'static str;

    /// Un-command name (e.g., "UNKLINE", "UNGLINE").
    fn unset_command_name(&self) -> &'static str;

    /// BanType for disconnect_matching_ban.
    fn ban_type(&self) -> BanType;

    /// Check if the user has the appropriate capability for this ban type.
    ///
    /// Returns `true` if the user is authorized, `false` otherwise.
    async fn check_capability(&self, authority: &CapabilityAuthority, uid: &str) -> bool;

    /// Add ban to database.
    async fn add_to_db(
        &self,
        db: &Database,
        target: &str,
        reason: &str,
        oper: &str,
    ) -> Result<(), DbError>;

    /// Remove ban from database.
    async fn remove_from_db(&self, db: &Database, target: &str) -> Result<bool, DbError>;

    /// Add ban to in-memory cache.
    async fn add_to_cache(&self, matrix: &Arc<Matrix>, target: &str, reason: &str, oper: &str);

    /// Remove ban from in-memory cache.
    async fn remove_from_cache(&self, matrix: &Arc<Matrix>, target: &str) -> bool;
}

// -----------------------------------------------------------------------------
// Generic Handlers
// -----------------------------------------------------------------------------

/// Generic handler for X-line add commands.
pub struct GenericBanAddHandler<C: BanConfig> {
    config: C,
}

impl<C: BanConfig> GenericBanAddHandler<C> {
    /// Create a new handler with the given config.
    pub const fn new(config: C) -> Self {
        Self { config }
    }
}

#[async_trait]
impl<C: BanConfig> PostRegHandler for GenericBanAddHandler<C> {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let cmd_name = self.config.command_name();
        let nick = ctx.nick();

        // Check capability via trait
        let authority = ctx.authority();
        if !self.config.check_capability(&authority, ctx.uid).await {
            let reply = Response::err_noprivileges(nick)
                .with_prefix(ctx.server_prefix());
            ctx.send_error(cmd_name, "ERR_NOPRIVILEGES", reply).await?;
            return Ok(());
        }

        // Parse target (mask/ip/pattern) - using require_arg_or_reply! here would need
        // a compile-time command string, but we have a dynamic cmd_name, so keep manual
        let target = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let reply = Response::err_needmoreparams(nick, cmd_name)
                    .with_prefix(ctx.server_prefix());
                ctx.send_error(cmd_name, "ERR_NEEDMOREPARAMS", reply).await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // Add to database
        if let Err(e) = self.config.add_to_db(ctx.db, target, reason, nick).await {
            tracing::error!(error = %e, "Failed to add {cmd_name} to database");
        }

        // Add to in-memory cache
        self.config
            .add_to_cache(ctx.matrix, target, reason, nick)
            .await;

        // Disconnect matching users
        let disconnected =
            disconnect_matching_ban(ctx, self.config.ban_type(), target, reason).await;

        tracing::info!(
            oper = %nick,
            target = %target,
            reason = %reason,
            disconnected = disconnected,
            cmd = cmd_name,
            "{} added", cmd_name
        );

        // Send confirmation
        let text = if disconnected > 0 {
            format!("{cmd_name} added: {target} ({reason}) - {disconnected} user(s) disconnected")
        } else {
            format!("{cmd_name} added: {target} ({reason})")
        };
        ctx.sender
            .send(server_notice(server_name, nick, &text))
            .await?;

        Ok(())
    }
}

/// Generic handler for X-line remove commands.
pub struct GenericBanRemoveHandler<C: BanConfig> {
    config: C,
}

impl<C: BanConfig> GenericBanRemoveHandler<C> {
    /// Create a new handler with the given config.
    pub const fn new(config: C) -> Self {
        Self { config }
    }
}

#[async_trait]
impl<C: BanConfig> PostRegHandler for GenericBanRemoveHandler<C> {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let cmd_name = self.config.unset_command_name();

        // Get nick and check capability
        let nick = ctx.nick();
        let authority = ctx.authority();
        if !self.config.check_capability(&authority, ctx.uid).await {
            let reply = Response::err_noprivileges(nick)
                .with_prefix(ctx.server_prefix());
            ctx.send_error(cmd_name, "ERR_NOPRIVILEGES", reply).await?;
            return Ok(());
        }

        // Parse target
        let target = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let reply = Response::err_needmoreparams(nick, cmd_name)
                    .with_prefix(ctx.server_prefix());
                ctx.send_error(cmd_name, "ERR_NEEDMOREPARAMS", reply).await?;
                return Ok(());
            }
        };

        // Remove from database
        let db_removed = match self.config.remove_from_db(ctx.db, target).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, cmd = cmd_name, "Failed to remove ban from database");
                false
            }
        };

        // Remove from in-memory cache
        let cache_removed = self.config.remove_from_cache(ctx.matrix, target).await;

        let removed = db_removed || cache_removed;
        if removed {
            tracing::info!(oper = %nick, target = %target, cmd = cmd_name, "{} removed", cmd_name);
        }

        // Send confirmation
        let text = if removed {
            format!("{} removed: {}", self.config.command_name(), target)
        } else {
            format!("No {} found for: {}", self.config.command_name(), target)
        };
        ctx.sender
            .send(server_notice(server_name, nick, &text))
            .await?;

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// IP Parsing Helper
// -----------------------------------------------------------------------------

/// Parse an IP address or CIDR string into an IpNet.
fn parse_ip_or_cidr(ip: &str) -> Option<IpNet> {
    ip.parse().ok().or_else(|| {
        // Try parsing as single IP and convert to /32 or /128
        // SAFETY: Prefix 32 (IPv4) and 128 (IPv6) are compile-time constants and always valid
        ip.parse::<std::net::IpAddr>().ok().map(|addr| match addr {
            std::net::IpAddr::V4(v4) => {
                IpNet::V4(ipnet::Ipv4Net::new(v4, 32).expect("prefix 32 is valid"))
            }
            std::net::IpAddr::V6(v6) => {
                IpNet::V6(ipnet::Ipv6Net::new(v6, 128).expect("prefix 128 is valid"))
            }
        })
    })
}

// -----------------------------------------------------------------------------
// Declarative Macro for Simple Ban Configs
// -----------------------------------------------------------------------------

/// Macro to define simple ban configurations with hostmask-based caching.
///
/// This eliminates ~100 lines of repetitive BanConfig implementations for
/// ban types that use the standard BanCache pattern (K/G-lines).
macro_rules! simple_ban_config {
    (
        $(#[$meta:meta])*
        $config_name:ident {
            command: $cmd:literal,
            unset_command: $unset_cmd:literal,
            ban_type: $ban_type:expr,
            capability_check: |$auth:ident, $uid:ident| $cap_check:expr,
            db_add: |$db_add:ident, $target_add:ident, $reason_add:ident, $oper_add:ident| $add_expr:expr,
            db_remove: |$db_rem:ident, $target_rem:ident| $remove_expr:expr,
            cache_add: |$cache_add:ident, $target_cache_add:ident, $reason_cache_add:ident| $cache_add_expr:expr,
            cache_remove: |$cache_rem:ident, $target_cache_rem:ident| $cache_remove_expr:expr,
        }
    ) => {
        $(#[$meta])*
        pub struct $config_name;

        #[async_trait]
        impl BanConfig for $config_name {
            fn command_name(&self) -> &'static str {
                $cmd
            }

            fn unset_command_name(&self) -> &'static str {
                $unset_cmd
            }

            fn ban_type(&self) -> BanType {
                $ban_type
            }

            async fn check_capability(&self, $auth: &CapabilityAuthority, $uid: &str) -> bool {
                $cap_check
            }

            async fn add_to_db(
                &self,
                $db_add: &Database,
                $target_add: &str,
                $reason_add: &str,
                $oper_add: &str,
            ) -> Result<(), DbError> {
                $add_expr
            }

            async fn remove_from_db(&self, $db_rem: &Database, $target_rem: &str) -> Result<bool, DbError> {
                $remove_expr
            }

            async fn add_to_cache(&self, $cache_add: &Arc<Matrix>, $target_cache_add: &str, $reason_cache_add: &str, _oper: &str) {
                $cache_add_expr
            }

            async fn remove_from_cache(&self, $cache_rem: &Arc<Matrix>, $target_cache_rem: &str) -> bool {
                $cache_remove_expr;
                true
            }
        }
    };
}

// -----------------------------------------------------------------------------
// K-line Config
// -----------------------------------------------------------------------------

simple_ban_config! {
    /// K-line (local user@host ban) configuration.
    KlineConfig {
        command: "KLINE",
        unset_command: "UNKLINE",
        ban_type: BanType::Kline,
        capability_check: |authority, uid| authority.request_kline_cap(uid).await.is_some(),
        db_add: |db, target, reason, oper| db.bans().add_kline(target, Some(reason), oper, None).await,
        db_remove: |db, target| db.bans().remove_kline(target).await,
        cache_add: |matrix, target, reason| matrix.security_manager.ban_cache.add_kline(target.to_string(), reason.to_string(), None),
        cache_remove: |matrix, target| matrix.security_manager.ban_cache.remove_kline(target),
    }
}

// -----------------------------------------------------------------------------
// G-line Config
// -----------------------------------------------------------------------------

simple_ban_config! {
    /// G-line (global user@host ban) configuration.
    GlineConfig {
        command: "GLINE",
        unset_command: "UNGLINE",
        ban_type: BanType::Gline,
        capability_check: |authority, uid| authority.request_gline_cap(uid).await.is_some(),
        db_add: |db, target, reason, oper| db.bans().add_gline(target, Some(reason), oper, None).await,
        db_remove: |db, target| db.bans().remove_gline(target).await,
        cache_add: |matrix, target, reason| matrix.security_manager.ban_cache.add_gline(target.to_string(), reason.to_string(), None),
        cache_remove: |matrix, target| matrix.security_manager.ban_cache.remove_gline(target),
    }
}

// -----------------------------------------------------------------------------
// Macro for IP-based Ban Configs (D-line, Z-line)
// -----------------------------------------------------------------------------

/// Macro to define IP-based ban configurations using IpDenyList.
///
/// D-lines and Z-lines use the IpDenyList (Roaring Bitmap) instead of BanCache.
macro_rules! ip_ban_config {
    (
        $(#[$meta:meta])*
        $config_name:ident {
            command: $cmd:literal,
            unset_command: $unset_cmd:literal,
            ban_type: $ban_type:expr,
            capability_check: |$auth:ident, $uid:ident| $cap_check:expr,
            db_add: |$db_add:ident, $target_add:ident, $reason_add:ident, $oper_add:ident| $add_expr:expr,
            db_remove: |$db_rem:ident, $target_rem:ident| $remove_expr:expr,
            log_prefix: $log_prefix:literal,
        }
    ) => {
        $(#[$meta])*
        pub struct $config_name;

        #[async_trait]
        impl BanConfig for $config_name {
            fn command_name(&self) -> &'static str {
                $cmd
            }

            fn unset_command_name(&self) -> &'static str {
                $unset_cmd
            }

            fn ban_type(&self) -> BanType {
                $ban_type
            }

            async fn check_capability(&self, $auth: &CapabilityAuthority, $uid: &str) -> bool {
                $cap_check
            }

            async fn add_to_db(
                &self,
                $db_add: &Database,
                $target_add: &str,
                $reason_add: &str,
                $oper_add: &str,
            ) -> Result<(), DbError> {
                $add_expr
            }

            async fn remove_from_db(&self, $db_rem: &Database, $target_rem: &str) -> Result<bool, DbError> {
                $remove_expr
            }

            async fn add_to_cache(&self, matrix: &Arc<Matrix>, target: &str, reason: &str, oper: &str) {
                if let Some(net) = parse_ip_or_cidr(target) {
                    if let Ok(mut deny_list) = matrix.security_manager.ip_deny_list.write()
                        && let Err(e) = deny_list.add_ban(net, reason.to_string(), None, oper.to_string())
                    {
                        tracing::error!(error = %e, concat!("Failed to add ", $log_prefix, " to IP deny list"));
                    }
                } else {
                    tracing::warn!(ip = %target, concat!($log_prefix, " IP could not be parsed as IP/CIDR"));
                }
            }

            async fn remove_from_cache(&self, matrix: &Arc<Matrix>, target: &str) -> bool {
                if let Some(net) = parse_ip_or_cidr(target)
                    && let Ok(mut deny_list) = matrix.security_manager.ip_deny_list.write()
                {
                    return deny_list.remove_ban(net).unwrap_or(false);
                }
                false
            }
        }
    };
}

// -----------------------------------------------------------------------------
// D-line Config (IP-based with IpDenyList)
// -----------------------------------------------------------------------------

ip_ban_config! {
    /// D-line (local IP ban) configuration.
    DlineConfig {
        command: "DLINE",
        unset_command: "UNDLINE",
        ban_type: BanType::Dline,
        capability_check: |authority, uid| authority.request_dline_cap(uid).await.is_some(),
        db_add: |db, target, reason, oper| db.bans().add_dline(target, Some(reason), oper, None).await,
        db_remove: |db, target| db.bans().remove_dline(target).await,
        log_prefix: "D-line",
    }
}

// -----------------------------------------------------------------------------
// Z-line Config (IP-based with IpDenyList)
// -----------------------------------------------------------------------------

ip_ban_config! {
    /// Z-line (global IP ban, skips DNS) configuration.
    ZlineConfig {
        command: "ZLINE",
        unset_command: "UNZLINE",
        ban_type: BanType::Zline,
        capability_check: |authority, uid| authority.request_zline_cap(uid).await.is_some(),
        db_add: |db, target, reason, oper| db.bans().add_zline(target, Some(reason), oper, None).await,
        db_remove: |db, target| db.bans().remove_zline(target).await,
        log_prefix: "Z-line",
    }
}

// -----------------------------------------------------------------------------
// R-line Config
// -----------------------------------------------------------------------------

simple_ban_config! {
    /// R-line (realname/GECOS ban) configuration.
    RlineConfig {
        command: "RLINE",
        unset_command: "UNRLINE",
        ban_type: BanType::Rline,
        capability_check: |authority, uid| authority.request_rline_cap(uid).await.is_some(),
        db_add: |db, target, reason, oper| db.bans().add_rline(target, Some(reason), oper, None).await,
        db_remove: |db, target| db.bans().remove_rline(target).await,
        cache_add: |_matrix, _target, _reason| {
            // R-lines don't have an in-memory cache (checked at connection time via DB)
        },
        cache_remove: |_matrix, _target| {
            // R-lines don't have an in-memory cache
        },
    }
}

// -----------------------------------------------------------------------------
// Type Aliases for Handlers
// -----------------------------------------------------------------------------

/// K-line add handler.
pub type KlineHandler = GenericBanAddHandler<KlineConfig>;
/// K-line remove handler.
pub type UnklineHandler = GenericBanRemoveHandler<KlineConfig>;

/// G-line add handler.
pub type GlineHandler = GenericBanAddHandler<GlineConfig>;
/// G-line remove handler.
pub type UnglineHandler = GenericBanRemoveHandler<GlineConfig>;

/// D-line add handler.
pub type DlineHandler = GenericBanAddHandler<DlineConfig>;
/// D-line remove handler.
pub type UndlineHandler = GenericBanRemoveHandler<DlineConfig>;

/// Z-line add handler.
pub type ZlineHandler = GenericBanAddHandler<ZlineConfig>;
/// Z-line remove handler.
pub type UnzlineHandler = GenericBanRemoveHandler<ZlineConfig>;

/// R-line add handler.
pub type RlineHandler = GenericBanAddHandler<RlineConfig>;
/// R-line remove handler.
pub type UnrlineHandler = GenericBanRemoveHandler<RlineConfig>;

// -----------------------------------------------------------------------------
// Constructor Functions (for Registry)
// -----------------------------------------------------------------------------

impl KlineHandler {
    /// Create a new K-line add handler.
    pub const fn kline() -> Self {
        Self::new(KlineConfig)
    }
}

impl UnklineHandler {
    /// Create a new K-line remove handler.
    pub const fn unkline() -> Self {
        Self::new(KlineConfig)
    }
}

impl GlineHandler {
    /// Create a new G-line add handler.
    pub const fn gline() -> Self {
        Self::new(GlineConfig)
    }
}

impl UnglineHandler {
    /// Create a new G-line remove handler.
    pub const fn ungline() -> Self {
        Self::new(GlineConfig)
    }
}

impl DlineHandler {
    /// Create a new D-line add handler.
    pub const fn dline() -> Self {
        Self::new(DlineConfig)
    }
}

impl UndlineHandler {
    /// Create a new D-line remove handler.
    pub const fn undline() -> Self {
        Self::new(DlineConfig)
    }
}

impl ZlineHandler {
    /// Create a new Z-line add handler.
    pub const fn zline() -> Self {
        Self::new(ZlineConfig)
    }
}

impl UnzlineHandler {
    /// Create a new Z-line remove handler.
    pub const fn unzline() -> Self {
        Self::new(ZlineConfig)
    }
}

impl RlineHandler {
    /// Create a new R-line add handler.
    pub const fn rline() -> Self {
        Self::new(RlineConfig)
    }
}

impl UnrlineHandler {
    /// Create a new R-line remove handler.
    pub const fn unrline() -> Self {
        Self::new(RlineConfig)
    }
}
