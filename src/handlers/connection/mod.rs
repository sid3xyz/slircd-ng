//! Connection and registration handlers.
//!
//! Handles NICK, USER, PASS, PING, PONG, QUIT, STARTTLS commands.

mod nick;
mod pass;
mod ping;
mod quit;
mod starttls;
mod user;
mod webirc;
mod welcome_burst;

pub use nick::NickHandler;
pub use pass::PassHandler;
pub use ping::{PingHandler, PongHandler};
pub use quit::QuitHandler;
pub use starttls::StarttlsHandler;
pub use user::UserHandler;
pub use webirc::WebircHandler;

use std::collections::HashMap;
use crate::handlers::PreRegHandler;
use crate::handlers::core::traits::DynUniversalHandler;
use crate::config::WebircBlock;

pub fn register(
    pre_reg: &mut HashMap<&'static str, Box<dyn PreRegHandler>>,
    universal: &mut HashMap<&'static str, Box<dyn DynUniversalHandler>>,
    webirc_blocks: Vec<WebircBlock>,
) {
    // Universal handlers
    universal.insert("QUIT", Box::new(QuitHandler));
    universal.insert("PING", Box::new(PingHandler));
    universal.insert("PONG", Box::new(PongHandler));
    universal.insert("NICK", Box::new(NickHandler));

    // Pre-registration handlers
    pre_reg.insert("WEBIRC", Box::new(WebircHandler::new(webirc_blocks)));
    pre_reg.insert("USER", Box::new(UserHandler));
    pre_reg.insert("PASS", Box::new(PassHandler));
    pre_reg.insert("STARTTLS", Box::new(StarttlsHandler));
}
pub use welcome_burst::WelcomeBurstWriter;
