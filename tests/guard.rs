//! The terminal guard (D-043): spawns the binary and checks that it refuses a
//! non-terminal stdin or stdout. Placeholder until T-23a; the guard times its
//! child, so the time types are allowed here.
#![allow(clippy::disallowed_types)]
