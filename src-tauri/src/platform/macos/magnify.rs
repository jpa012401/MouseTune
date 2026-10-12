//! Zoom steps (Phase 6 design §3). Until the magnify gesture is verified
//! (Task 11) every step is the documented fallback: ⌘= to zoom in, ⌘- to
//! zoom out.

use crate::{
    actions::keys::{KeyBinding, ModifierMask, key_combo_strokes},
    actions::perform::EventPoster,
    input::intents::MagnifyOutput,
    platform::event_poster::MacEventPoster,
};

const EQUALS: u16 = 0x18;
const MINUS: u16 = 0x1B;

pub struct MacMagnify;

impl MagnifyOutput for MacMagnify {
    fn step(&mut self, amount: f64) {
        let key_code = if amount > 0.0 { EQUALS } else { MINUS };
        let strokes = key_combo_strokes(KeyBinding {
            key_code,
            modifiers: ModifierMask::COMMAND,
        });
        let _ = MacEventPoster.post_keys(&strokes);
    }
}
