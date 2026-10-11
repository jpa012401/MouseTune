//! Input engine core (spec §4). Pure logic; macOS adapters live in
//! `platform::macos`.

// Nothing outside tests uses the engine until it's wired in (Task 8),
// which removes this allow.
#![allow(dead_code)]

pub mod devices;
pub mod engine;
pub mod filter;
pub mod guard;
pub mod taps;
