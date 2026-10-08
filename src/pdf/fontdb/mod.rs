//! Font DB: AGL, glyph maps, decoding through our own fonts, inference, builders.

pub mod build;

pub(crate) mod agl;
pub(crate) mod decode;
pub(crate) mod gmap;
pub(crate) mod score;
pub(crate) mod template;
