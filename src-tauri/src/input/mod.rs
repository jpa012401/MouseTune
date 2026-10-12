//! Input engine core (spec §4). Pure logic; macOS adapters live in
//! `platform::macos`.

pub mod capture;
pub mod click_cycle;
pub mod devices;
pub mod drag;
pub mod engine;
pub mod filter;
pub mod guard;
pub mod intents;
pub mod key_recorder;
pub mod remaps;
pub mod router;
pub mod scroll;
pub mod taps;
pub mod thread;
