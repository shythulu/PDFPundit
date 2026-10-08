//! The terminal UI. The director animates and the loop polls, so the time types
//! are allowed under `ui/`; nothing here reaches an artefact.
#![allow(clippy::disallowed_types)]

pub(crate) mod app;
pub(crate) mod canvas;
pub(crate) mod cat;
pub(crate) mod color;
pub(crate) mod director;
#[cfg(test)]
pub(crate) mod goldens;
pub(crate) mod input;
pub(crate) mod layout;
pub(crate) mod state;
pub(crate) mod strings;
pub(crate) mod term;
pub(crate) mod theme;
pub(crate) mod view;
pub(crate) mod widgets;
