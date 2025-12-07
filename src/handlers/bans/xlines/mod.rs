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
use crate::handlers::{
    Context, Handler, HandlerResult, err_needmoreparams, err_noprivileges, get_nick_or_star,
    server_notice,
};
use crate::state::Matrix;
use async_trait::async_trait;
use ipnet::IpNet;
use slirc_proto::MessageRef;
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

    /// Argument name for logging (e.g., "mask", "ip", "pattern").
    /// Reserved for future extended logging.
    #[allow(dead_code)]
    fn arg_name(&self) -> &'static str;

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
impl<C: BanConfig> Handler for GenericBanAddHandler<C> {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;
        let cmd_name = self.config.command_name();

        // Get nick and check capability
        let nick = get_nick_or_star(ctx).await;
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        if !self.config.check_capability(&authority, ctx.uid).await {
            ctx.sender
                .send(err_noprivileges(server_name, &nick))
                .await?;
            return Ok(());
        }

        // Parse target (mask/ip/pattern)
        let target = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, cmd_name))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // Add to database
        if let Err(e) = self.config.add_to_db(ctx.db, target, reason, &nick).await {
            tracing::error!(error = %e, "Failed to add {cmd_name} to database");
        }

        // Add to in-memory cache
        self.config
            .add_to_cache(ctx.matrix, target, reason, &nick)
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
            .send(server_notice(server_name, &nick, &text))
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
impl<C: BanConfig> Handler for GenericBanRemoveHandler<C> {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;
        let cmd_name = self.config.unset_command_name();

        // Get nick and check capability
        let nick = get_nick_or_star(ctx).await;
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        if !self.config.check_capability(&authority, ctx.uid).await {
            ctx.sender
                .send(err_noprivileges(server_name, &nick))
                .await?;
            return Ok(());
        }

        // Parse target
        let target = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, cmd_name))
                    .await?;
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
            .send(server_notice(server_name, &nick, &text))
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
// K-line Config
// -----------------------------------------------------------------------------

/// K-line (local user@host ban) configuration.
pub struct KlineConfig;

#[async_trait]
impl BanConfig for KlineConfig {
    fn command_name(&self) -> &'static str {
        "KLINE"
    }

    fn unset_command_name(&self) -> &'static str {
        "UNKLINE"
    }

    fn arg_name(&self) -> &'static str {
        "mask"
    }

    fn ban_type(&self) -> BanType {
        BanType::Kline
    }

    async fn check_capability(&self, authority: &CapabilityAuthority, uid: &str) -> bool {
        authority.request_kline_cap(uid).await.is_some()
    }

    async fn add_to_db(
        &self,
        db: &Database,
        target: &str,
        reason: &str,
        oper: &str,
    ) -> Result<(), DbError> {
        db.bans().add_kline(target, Some(reason), oper, None).await
    }

    async fn remove_from_db(&self, db: &Database, target: &str) -> Result<bool, DbError> {
        db.bans().remove_kline(target).await
    }

    async fn add_to_cache(&self, matrix: &Arc<Matrix>, target: &str, reason: &str, _oper: &str) {
        matrix
            .ban_cache
            .add_kline(target.to_string(), reason.to_string(), None);
    }

    async fn remove_from_cache(&self, matrix: &Arc<Matrix>, target: &str) -> bool {
        matrix.ban_cache.remove_kline(target);
        true
    }
}

// -----------------------------------------------------------------------------
// G-line Config
// -----------------------------------------------------------------------------

/// G-line (global user@host ban) configuration.
pub struct GlineConfig;

#[async_trait]
impl BanConfig for GlineConfig {
    fn command_name(&self) -> &'static str {
        "GLINE"
    }

    fn unset_command_name(&self) -> &'static str {
        "UNGLINE"
    }

    fn arg_name(&self) -> &'static str {
        "mask"
    }

    fn ban_type(&self) -> BanType {
        BanType::Gline
    }

    async fn check_capability(&self, authority: &CapabilityAuthority, uid: &str) -> bool {
        authority.request_gline_cap(uid).await.is_some()
    }

    async fn add_to_db(
        &self,
        db: &Database,
        target: &str,
        reason: &str,
        oper: &str,
    ) -> Result<(), DbError> {
        db.bans().add_gline(target, Some(reason), oper, None).await
    }

    async fn remove_from_db(&self, db: &Database, target: &str) -> Result<bool, DbError> {
        db.bans().remove_gline(target).await
    }

    async fn add_to_cache(&self, matrix: &Arc<Matrix>, target: &str, reason: &str, _oper: &str) {
        matrix
            .ban_cache
            .add_gline(target.to_string(), reason.to_string(), None);
    }

    async fn remove_from_cache(&self, matrix: &Arc<Matrix>, target: &str) -> bool {
        matrix.ban_cache.remove_gline(target);
        true
    }
}

// -----------------------------------------------------------------------------
// D-line Config (IP-based with IpDenyList)
// -----------------------------------------------------------------------------

/// D-line (local IP ban) configuration.
pub struct DlineConfig;

#[async_trait]
impl BanConfig for DlineConfig {
    fn command_name(&self) -> &'static str {
        "DLINE"
    }

    fn unset_command_name(&self) -> &'static str {
        "UNDLINE"
    }

    fn arg_name(&self) -> &'static str {
        "ip"
    }

    fn ban_type(&self) -> BanType {
        BanType::Dline
    }

    async fn check_capability(&self, authority: &CapabilityAuthority, uid: &str) -> bool {
        authority.request_dline_cap(uid).await.is_some()
    }

    async fn add_to_db(
        &self,
        db: &Database,
        target: &str,
        reason: &str,
        oper: &str,
    ) -> Result<(), DbError> {
        db.bans().add_dline(target, Some(reason), oper, None).await
    }

    async fn remove_from_db(&self, db: &Database, target: &str) -> Result<bool, DbError> {
        db.bans().remove_dline(target).await
    }

    async fn add_to_cache(&self, matrix: &Arc<Matrix>, target: &str, reason: &str, oper: &str) {
        if let Some(net) = parse_ip_or_cidr(target) {
            if let Ok(mut deny_list) = matrix.ip_deny_list.write()
                && let Err(e) = deny_list.add_ban(net, reason.to_string(), None, oper.to_string())
            {
                tracing::error!(error = %e, "Failed to add D-line to IP deny list");
            }
        } else {
            tracing::warn!(ip = %target, "D-line IP could not be parsed as IP/CIDR");
        }
    }

    async fn remove_from_cache(&self, matrix: &Arc<Matrix>, target: &str) -> bool {
        if let Some(net) = parse_ip_or_cidr(target)
            && let Ok(mut deny_list) = matrix.ip_deny_list.write()
        {
            return deny_list.remove_ban(net).unwrap_or(false);
        }
        false
    }
}

// -----------------------------------------------------------------------------
// Z-line Config (IP-based with IpDenyList)
// -----------------------------------------------------------------------------

/// Z-line (global IP ban, skips DNS) configuration.
pub struct ZlineConfig;

#[async_trait]
impl BanConfig for ZlineConfig {
    fn command_name(&self) -> &'static str {
        "ZLINE"
    }

    fn unset_command_name(&self) -> &'static str {
        "UNZLINE"
    }

    fn arg_name(&self) -> &'static str {
        "ip"
    }

    fn ban_type(&self) -> BanType {
        BanType::Zline
    }

    async fn check_capability(&self, authority: &CapabilityAuthority, uid: &str) -> bool {
        authority.request_zline_cap(uid).await.is_some()
    }

    async fn add_to_db(
        &self,
        db: &Database,
        target: &str,
        reason: &str,
        oper: &str,
    ) -> Result<(), DbError> {
        db.bans().add_zline(target, Some(reason), oper, None).await
    }

    async fn remove_from_db(&self, db: &Database, target: &str) -> Result<bool, DbError> {
        db.bans().remove_zline(target).await
    }

    async fn add_to_cache(&self, matrix: &Arc<Matrix>, target: &str, reason: &str, oper: &str) {
        if let Some(net) = parse_ip_or_cidr(target) {
            if let Ok(mut deny_list) = matrix.ip_deny_list.write()
                && let Err(e) = deny_list.add_ban(net, reason.to_string(), None, oper.to_string())
            {
                tracing::error!(error = %e, "Failed to add Z-line to IP deny list");
            }
        } else {
            tracing::warn!(ip = %target, "Z-line IP could not be parsed as IP/CIDR");
        }
    }

    async fn remove_from_cache(&self, matrix: &Arc<Matrix>, target: &str) -> bool {
        if let Some(net) = parse_ip_or_cidr(target)
            && let Ok(mut deny_list) = matrix.ip_deny_list.write()
        {
            return deny_list.remove_ban(net).unwrap_or(false);
        }
        false
    }
}

// -----------------------------------------------------------------------------
// R-line Config
// -----------------------------------------------------------------------------

/// R-line (realname/GECOS ban) configuration.
pub struct RlineConfig;

#[async_trait]
impl BanConfig for RlineConfig {
    fn command_name(&self) -> &'static str {
        "RLINE"
    }

    fn unset_command_name(&self) -> &'static str {
        "UNRLINE"
    }

    fn arg_name(&self) -> &'static str {
        "pattern"
    }

    fn ban_type(&self) -> BanType {
        BanType::Rline
    }

    async fn check_capability(&self, authority: &CapabilityAuthority, uid: &str) -> bool {
        authority.request_rline_cap(uid).await.is_some()
    }

    async fn add_to_db(
        &self,
        db: &Database,
        target: &str,
        reason: &str,
        oper: &str,
    ) -> Result<(), DbError> {
        db.bans().add_rline(target, Some(reason), oper, None).await
    }

    async fn remove_from_db(&self, db: &Database, target: &str) -> Result<bool, DbError> {
        db.bans().remove_rline(target).await
    }

    async fn add_to_cache(&self, _matrix: &Arc<Matrix>, _target: &str, _reason: &str, _oper: &str) {
        // R-lines don't have an in-memory cache (checked at connection time via DB)
    }

    async fn remove_from_cache(&self, _matrix: &Arc<Matrix>, _target: &str) -> bool {
        // R-lines don't have an in-memory cache
        true
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
