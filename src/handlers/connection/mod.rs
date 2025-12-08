//! Connection and registration handlers.
//!
//! Handles NICK, USER, PASS, PING, PONG, QUIT commands.

mod caps;
mod nick;
mod pass;
mod ping;
mod quit;
mod user;
mod webirc;
mod welcome;

pub use nick::NickHandler;
pub use pass::PassHandler;
pub use ping::{PingHandler, PongHandler};
pub use quit::QuitHandler;
pub use user::UserHandler;
pub use webirc::WebircHandler;
