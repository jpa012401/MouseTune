//! Input engine core (spec §4). Pure logic; macOS adapters live in
//! `platform::macos`.

pub mod devices;
pub mod engine;
pub mod filter;
pub mod guard;
pub mod taps;
pub mod thread;
