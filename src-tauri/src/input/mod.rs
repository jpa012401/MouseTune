//! Input engine core (spec §4). Pure logic; macOS adapters live in
//! `platform::macos`.

pub mod devices;
pub mod engine;
pub mod filter;
pub mod guard;
#[allow(dead_code)] // Used from Phase 4a Task 4.
pub mod remaps;
pub mod taps;
pub mod thread;
