//! Zoom steps (Phase 6 design §3). Until the magnify gesture is verified
//! (Task 11) every step is the documented fallback: ⌘= to zoom in, ⌘- to
//! zoom out.
//!
//! The fallback posts only the `=` / `-` key, with the Command flag on its
//! down and up, and never a separate ⌘ key down or up: when the zoom key is
//! bound to ⌘ the user is physically holding ⌘, and a synthetic ⌘ up could
//! clear it from the session's flag state, so the next ticks would scroll
//! instead of zooming.

use crate::{
    actions::keys::{KeyStroke, ModifierMask},
    actions::perform::EventPoster,
    input::intents::MagnifyOutput,
    platform::event_poster::MacEventPoster,
};

const EQUALS: u16 = 0x18;
const MINUS: u16 = 0x1B;

/// The key fallback for one step: `=` (in) or `-` (out), down then up,
/// each carrying the Command flag. No ⌘ key events (module doc).
pub fn zoom_key_strokes(amount: f64) -> [KeyStroke; 2] {
    let key_code = if amount > 0.0 { EQUALS } else { MINUS };
    [true, false].map(|down| KeyStroke {
        key_code,
        down,
        flags: ModifierMask::COMMAND,
    })
}

pub struct MacMagnify;

impl MagnifyOutput for MacMagnify {
    fn step(&mut self, amount: f64) {
        let _ = MacEventPoster.post_keys(&zoom_key_strokes(amount));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn stroke(key_code: u16, down: bool) -> KeyStroke {
        KeyStroke {
            key_code,
            down,
            flags: ModifierMask(0x0010_0000),
        }
    }

    #[test]
    fn zooming_in_posts_only_the_equals_key_with_the_command_flag() {
        let strokes = zoom_key_strokes(1.0);
        assert_eq!(strokes, [stroke(0x18, true), stroke(0x18, false)]);
        assert!(strokes.iter().all(|s| s.key_code != 0x37));
    }

    #[test]
    fn zooming_out_posts_only_the_minus_key_with_the_command_flag() {
        let strokes = zoom_key_strokes(-1.0);
        assert_eq!(strokes, [stroke(0x1B, true), stroke(0x1B, false)]);
        assert!(strokes.iter().all(|s| s.key_code != 0x37));
    }
}
