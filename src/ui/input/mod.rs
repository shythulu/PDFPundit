//! Terminal input: app-owned tty input after a kitty handshake, crossterm otherwise (D-033).

pub(crate) mod collector;
pub(crate) mod keys;
pub(crate) mod osc72;
pub(crate) mod paste;
