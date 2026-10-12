//! Smooth scrolling (Phase 5a design, spec §8). Pure core; the display link
//! and the event poster are ports implemented in `platform::macos`.

pub mod acceleration;
pub mod curves;
