//! NickServ - Nickname registration and identification service.
//!
//! Handles:
//! - `REGISTER <password> [email]` - Register current nick
//! - `IDENTIFY <password>` - Identify to account
//! - `GHOST <nick>` - Kill session using your nick
//! - `INFO <nick>` - Show account information
//! - `SET <option> <value>` - Configure account settings

mod commands;

pub use commands::NickServ;
