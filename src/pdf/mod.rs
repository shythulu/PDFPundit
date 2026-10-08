//! The repair engine's internals. Order reaches output here, so iterating a hash
//! container is denied.
#![deny(clippy::iter_over_hash_type)]

pub mod fixtures;
pub mod fontdb;
pub mod model;
pub mod write;

pub(crate) mod carver;
pub(crate) mod diagnose;
pub(crate) mod emit;
#[cfg(feature = "export")]
pub(crate) mod export;
pub(crate) mod graph;
pub(crate) mod lexer;
pub(crate) mod meta;
#[cfg(feature = "ocr")]
pub(crate) mod ocr;
pub(crate) mod plan;
pub(crate) mod rebuild;
pub(crate) mod repair;
pub(crate) mod streams;
pub(crate) mod text;
pub(crate) mod verify;
