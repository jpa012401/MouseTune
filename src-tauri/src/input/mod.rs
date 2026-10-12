//! Input engine core (spec §4). Pure logic; macOS adapters live in
//! `platform::macos`.

pub mod capture;
pub mod click_cycle;
pub mod devices;
pub mod engine;
pub mod filter;
pub mod guard;
pub mod key_recorder;
pub mod remaps;
pub mod router;
#[allow(dead_code)] // Wired into the engine in Phase 5a Task 8.
pub mod scroll;
pub mod taps;
pub mod thread;
