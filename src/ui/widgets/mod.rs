//! Widgets drawn into the Canvas (T-22a, T-22b): the pieces the full layout
//! is built from.
#![cfg_attr(not(test), allow(dead_code))]

// `box` is a reserved word; the file keeps the ticket's name.
pub(crate) mod r#box;
pub(crate) mod lightbar;
pub(crate) mod logo;
