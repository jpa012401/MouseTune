//! Key recording (Phase 4b design): while recording, the first real
//! (non-modifier) key-down is reported with its modifiers and swallowed, so
//! recording ⌘Q quits nothing. Esc alone cancels. A key-up is swallowed only
//! if its down was. Pure and allocation-free: `on_key` runs in the keys tap
//! callback.

use std::time::Instant;

use crate::{
    actions::{Notice, RawKeyCapture, keys::ModifierMask, keys::is_modifier_key},
    input::{capture::CAPTURE_TIMEOUT, filter::MOUSETUNE_EVENT_TAG, router::Verdict},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Down,
    Up,
}

/// A key event as the keys tap copies it out. `chars` are UTF-16 units,
/// `chars_len` of them valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub kind: KeyKind,
    pub key_code: u16,
    pub flags: u64,
    pub chars: [u16; 4],
    pub chars_len: u8,
    pub user_data: i64,
}

/// macOS virtual key code of Esc.
pub const ESCAPE: u16 = 0x35;

/// The modifier flags a recorded shortcut keeps; device-dependent bits are dropped.
const RECORDED_MODIFIERS: ModifierMask = ModifierMask(
    ModifierMask::SHIFT.0
        | ModifierMask::CONTROL.0
        | ModifierMask::OPTION.0
        | ModifierMask::COMMAND.0,
);

#[derive(Debug, Default)]
pub struct KeyRecorder {
    until: Option<Instant>,
    /// Bit n set: key code n's down was swallowed, so its up is too.
    swallowed: u128,
}

impl KeyRecorder {
    pub fn start(&mut self, now: Instant) {
        self.until = now.checked_add(CAPTURE_TIMEOUT);
    }

    pub fn stop(&mut self) {
        self.until = None;
    }

    /// Recording, or a swallowed key still down.
    pub fn wants_tap(&self) -> bool {
        self.until.is_some() || self.swallowed != 0
    }

    /// The keys tap missed events, so a swallowed key's up may already have
    /// reached the OS.
    pub fn forget_swallowed(&mut self) {
        self.swallowed = 0;
    }

    pub fn on_key(&mut self, e: &KeyEvent, now: Instant) -> (Verdict, Option<Notice>) {
        // Our own posted keys are never recorded; codes past 127 don't fit
        // the swallowed bitset, so they are left alone.
        if e.user_data == MOUSETUNE_EVENT_TAG || e.key_code >= 128 {
            return (Verdict::Pass, None);
        }
        let bit = 1u128 << e.key_code;
        match e.kind {
            KeyKind::Up => {
                if self.swallowed & bit != 0 {
                    self.swallowed &= !bit;
                    (Verdict::Swallow, None)
                } else {
                    (Verdict::Pass, None)
                }
            }
            KeyKind::Down => {
                if self.swallowed & bit != 0 {
                    return (Verdict::Swallow, None); // autorepeat
                }
                let Some(until) = self.until else {
                    return (Verdict::Pass, None);
                };
                if now > until {
                    self.until = None;
                    return (Verdict::Pass, Some(Notice::KeyCaptureCancelled));
                }
                if is_modifier_key(e.key_code) {
                    return (Verdict::Pass, None);
                }
                self.until = None;
                self.swallowed |= bit;
                let modifiers = ModifierMask(e.flags & RECORDED_MODIFIERS.0);
                if e.key_code == ESCAPE && modifiers == ModifierMask(0) {
                    return (Verdict::Swallow, Some(Notice::KeyCaptureCancelled));
                }
                (
                    Verdict::Swallow,
                    Some(Notice::KeyCaptured(RawKeyCapture {
                        key_code: e.key_code,
                        modifiers,
                        chars: e.chars,
                        chars_len: e.chars_len.min(4),
                    })),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::OnceLock, time::Duration};

    use super::*;

    const CMD: u64 = 0x100000;
    const SHIFT: u64 = 0x20000;

    fn t() -> Instant {
        static BASE: OnceLock<Instant> = OnceLock::new();
        *BASE.get_or_init(Instant::now)
    }

    fn down(key_code: u16, flags: u64, ch: char) -> KeyEvent {
        KeyEvent {
            kind: KeyKind::Down,
            key_code,
            flags,
            chars: [ch as u16, 0, 0, 0],
            chars_len: 1,
            user_data: 0,
        }
    }

    fn up(key_code: u16) -> KeyEvent {
        KeyEvent {
            kind: KeyKind::Up,
            key_code,
            flags: 0,
            chars: [0; 4],
            chars_len: 0,
            user_data: 0,
        }
    }

    #[test]
    fn when_idle_every_key_passes() {
        let mut r = KeyRecorder::default();
        assert_eq!(r.on_key(&down(0x01, 0, 's'), t()), (Verdict::Pass, None));
        assert!(!r.wants_tap());
    }

    #[test]
    fn the_first_real_key_is_recorded_with_its_modifiers_and_swallowed() {
        let mut r = KeyRecorder::default();
        r.start(t());
        assert!(r.wants_tap());
        assert_eq!(
            r.on_key(&down(0x01, CMD | SHIFT | 0x100, 's'), t()),
            (
                Verdict::Swallow,
                Some(Notice::KeyCaptured(RawKeyCapture {
                    key_code: 0x01,
                    modifiers: ModifierMask(0x120000),
                    chars: [0x73, 0, 0, 0],
                    chars_len: 1
                }))
            )
        );
        assert!(r.wants_tap(), "still waiting for the key's up");
        assert_eq!(r.on_key(&up(0x01), t()), (Verdict::Swallow, None));
        assert!(!r.wants_tap());
    }

    #[test]
    fn recording_command_q_swallows_it_and_records_it() {
        let mut r = KeyRecorder::default();
        r.start(t());
        let (verdict, notice) = r.on_key(&down(0x0C, CMD, 'q'), t());
        assert_eq!(verdict, Verdict::Swallow);
        assert!(
            matches!(notice, Some(Notice::KeyCaptured(c)) if c.key_code == 0x0C && c.modifiers == ModifierMask(CMD))
        );
    }

    #[test]
    fn escape_alone_cancels_and_is_swallowed() {
        let mut r = KeyRecorder::default();
        r.start(t());
        assert_eq!(
            r.on_key(&down(ESCAPE, 0, '\u{1b}'), t()),
            (Verdict::Swallow, Some(Notice::KeyCaptureCancelled))
        );
        assert_eq!(r.on_key(&up(ESCAPE), t()), (Verdict::Swallow, None));
        assert_eq!(r.on_key(&down(0x01, 0, 's'), t()), (Verdict::Pass, None));
    }

    #[test]
    fn escape_with_a_modifier_is_recorded() {
        let mut r = KeyRecorder::default();
        r.start(t());
        assert!(matches!(
            r.on_key(&down(ESCAPE, CMD, '\u{1b}'), t()).1,
            Some(Notice::KeyCaptured(_))
        ));
    }

    #[test]
    fn a_key_held_before_recording_passes_its_up() {
        let mut r = KeyRecorder::default();
        r.start(t());
        assert_eq!(r.on_key(&up(0x02), t()), (Verdict::Pass, None));
    }

    #[test]
    fn autorepeat_of_the_recorded_key_is_swallowed() {
        let mut r = KeyRecorder::default();
        r.start(t());
        r.on_key(&down(0x01, 0, 's'), t());
        assert_eq!(r.on_key(&down(0x01, 0, 's'), t()), (Verdict::Swallow, None));
    }

    #[test]
    fn a_second_key_after_recording_passes() {
        let mut r = KeyRecorder::default();
        r.start(t());
        r.on_key(&down(0x01, 0, 's'), t());
        assert_eq!(r.on_key(&down(0x02, 0, 'd'), t()), (Verdict::Pass, None));
    }

    #[test]
    fn after_the_deadline_the_next_key_passes_and_recording_is_cancelled() {
        let mut r = KeyRecorder::default();
        r.start(t());
        assert_eq!(
            r.on_key(&down(0x01, 0, 's'), t() + Duration::from_millis(10_001)),
            (Verdict::Pass, Some(Notice::KeyCaptureCancelled))
        );
        assert!(!r.wants_tap());
    }

    #[test]
    fn our_own_posted_keys_pass_while_recording() {
        let mut r = KeyRecorder::default();
        r.start(t());
        let tagged = KeyEvent {
            user_data: MOUSETUNE_EVENT_TAG,
            ..down(0x01, 0, 's')
        };
        assert_eq!(r.on_key(&tagged, t()), (Verdict::Pass, None));
        assert!(r.wants_tap());
    }

    #[test]
    fn stop_ends_recording() {
        let mut r = KeyRecorder::default();
        r.start(t());
        r.stop();
        assert_eq!(r.on_key(&down(0x01, 0, 's'), t()), (Verdict::Pass, None));
    }

    #[test]
    fn forgetting_lets_a_swallowed_key_up_through() {
        let mut r = KeyRecorder::default();
        r.start(t());
        r.on_key(&down(0x01, 0, 's'), t());
        r.forget_swallowed();
        assert_eq!(r.on_key(&up(0x01), t()), (Verdict::Pass, None));
        assert!(!r.wants_tap());
    }

    #[test]
    fn a_modifier_key_down_never_ends_recording() {
        let mut r = KeyRecorder::default();
        r.start(t());
        assert_eq!(r.on_key(&down(0x37, CMD, '\0'), t()), (Verdict::Pass, None));
        assert!(matches!(
            r.on_key(&down(0x01, CMD, 's'), t()).1,
            Some(Notice::KeyCaptured(_))
        ));
    }

    #[test]
    fn key_codes_past_127_pass() {
        let mut r = KeyRecorder::default();
        r.start(t());
        assert_eq!(r.on_key(&down(200, 0, 'x'), t()), (Verdict::Pass, None));
        assert!(r.wants_tap());
    }
}
