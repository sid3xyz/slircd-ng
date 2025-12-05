//! Connection and registration handlers.
//!
//! Handles NICK, USER, PASS, PING, PONG, QUIT commands.

mod nick;
mod user;
mod pass;
mod webirc;
mod welcome;
mod ping;
mod quit;
mod caps;

pub use nick::NickHandler;
pub use user::UserHandler;
pub use pass::PassHandler;
pub use webirc::WebircHandler;
pub use ping::{PingHandler, PongHandler};
pub use quit::QuitHandler;
pub use welcome::send_welcome_burst;
