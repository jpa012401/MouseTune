//! Action execution (spec §7, design §3). The input engine submits `Job`s
//! through `ActionSink`; `mt-exec` performs them against OS ports.

pub mod executor;
pub mod keys;
pub mod perform;
pub mod queue;

use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;
use specta::Type;

use self::keys::{KeyBinding, ModifierMask};
use crate::config::schema::{Action, KeyModifier, NavDirection, SymbolicHotkeyId};

/// The engine's copy of an action: plain data the tap callback can copy
/// without touching the heap. Built by `input::remaps::compile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledAction {
    SymbolicHotkey(SymbolicHotkeyId),
    Navigate(NavDirection),
    KeyCombo(KeyBinding),
}

impl From<&Action> for CompiledAction {
    fn from(action: &Action) -> Self {
        match action {
            Action::SymbolicHotkey { id } => Self::SymbolicHotkey(*id),
            Action::Navigate { direction } => Self::Navigate(*direction),
            Action::KeyCombo {
                key_code,
                modifiers,
                ..
            } => Self::KeyCombo(KeyBinding {
                key_code: *key_code,
                modifiers: modifier_mask(modifiers),
            }),
        }
    }
}

/// The CGEventFlags for a list of modifier names.
pub fn modifier_mask(modifiers: &[KeyModifier]) -> ModifierMask {
    modifiers.iter().fold(ModifierMask(0), |mask, m| {
        mask | match m {
            KeyModifier::Command => ModifierMask::COMMAND,
            KeyModifier::Shift => ModifierMask::SHIFT,
            KeyModifier::Option => ModifierMask::OPTION,
            KeyModifier::Control => ModifierMask::CONTROL,
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Perform(CompiledAction),
    /// A plain click of this CG button number.
    Click {
        button: u8,
    },
    /// A result for the settings UI, not an action.
    Notice(Notice),
}

/// Results that leave the tap callbacks for the UI. Plain copies, so the
/// hot path builds them without the heap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// 1-based, as the config and UI count buttons (3 = middle).
    ButtonCaptured {
        button: u8,
    },
    KeyCaptured(RawKeyCapture),
    KeyCaptureCancelled,
}

/// A captured key as the key tap copies it out: the typed characters are
/// UTF-16 units, `chars_len` of them valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawKeyCapture {
    pub key_code: u16,
    pub modifiers: ModifierMask,
    pub chars: [u16; 4],
    pub chars_len: u8,
}

/// A captured key as the UI shows it, built on `mt-exec`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCapture {
    pub key_code: u16,
    pub modifiers: Vec<KeyModifier>,
    pub key_label: String,
}

/// Port: where `mt-exec` hands notices (Tauri events, from Phase 4b Task 7).
pub trait NoticeObserver: Send {
    fn button_captured(&self, button: u8);
    fn key_captured(&self, capture: KeyCapture);
    fn key_capture_cancelled(&self);
}

/// Turns a notice into its observer call. Runs on `mt-exec`, so it may allocate.
pub fn deliver(notice: Notice, observer: &dyn NoticeObserver) {
    match notice {
        Notice::ButtonCaptured { button } => observer.button_captured(button),
        Notice::KeyCaptured(raw) => {
            let chars = &raw.chars[..usize::from(raw.chars_len).min(4)];
            observer.key_captured(KeyCapture {
                key_code: raw.key_code,
                modifiers: modifier_names(raw.modifiers),
                key_label: keys::key_label(raw.key_code, chars),
            });
        }
        Notice::KeyCaptureCancelled => observer.key_capture_cancelled(),
    }
}

/// The modifier names in a mask, in display order (⌃⌥⇧⌘).
fn modifier_names(mask: ModifierMask) -> Vec<KeyModifier> {
    [
        (ModifierMask::CONTROL, KeyModifier::Control),
        (ModifierMask::OPTION, KeyModifier::Option),
        (ModifierMask::SHIFT, KeyModifier::Shift),
        (ModifierMask::COMMAND, KeyModifier::Command),
    ]
    .into_iter()
    .filter(|(flag, _)| mask.contains(*flag))
    .map(|(_, name)| name)
    .collect()
}

/// Port the input engine submits through. `false` = dropped (queue full or closed).
pub trait ActionSink {
    fn submit(&self, job: Job) -> bool;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Type)]
pub struct ActionStats {
    pub performed: u32,
    pub dropped: u32,
    pub failed: u32,
}

#[derive(Debug, Default)]
pub struct ActionCounters {
    performed: AtomicU32,
    dropped: AtomicU32,
    failed: AtomicU32,
}

fn bump(counter: &AtomicU32) {
    // fetch_update with saturating_add: never wraps to 0.
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
        Some(n.saturating_add(1))
    });
}

impl ActionCounters {
    pub fn record_performed(&self) {
        bump(&self.performed);
    }
    pub fn record_dropped(&self) {
        bump(&self.dropped);
    }
    pub fn record_failed(&self) {
        bump(&self.failed);
    }
    pub fn stats(&self) -> ActionStats {
        ActionStats {
            performed: self.performed.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OutputError {
    #[error("{0} failed")]
    Os(&'static str),
    #[error("no shortcut to post")]
    NoShortcut,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_actions_compile_to_plain_copies() {
        use crate::config::schema::{Action, NavDirection, SymbolicHotkeyId};
        assert_eq!(
            CompiledAction::from(&Action::SymbolicHotkey {
                id: SymbolicHotkeyId::MissionControl
            }),
            CompiledAction::SymbolicHotkey(SymbolicHotkeyId::MissionControl)
        );
        assert_eq!(
            CompiledAction::from(&Action::Navigate {
                direction: NavDirection::Back
            }),
            CompiledAction::Navigate(NavDirection::Back)
        );
    }

    #[test]
    fn counters_report_each_kind_separately() {
        let c = ActionCounters::default();
        c.record_performed();
        c.record_performed();
        c.record_dropped();
        c.record_failed();
        assert_eq!(
            c.stats(),
            ActionStats {
                performed: 2,
                dropped: 1,
                failed: 1
            }
        );
    }

    #[test]
    fn a_key_combo_compiles_to_its_key_and_modifier_mask() {
        use crate::config::schema::KeyModifier;
        let action = Action::KeyCombo {
            key_code: 1,
            modifiers: vec![KeyModifier::Command, KeyModifier::Shift],
            key_label: "S".into(),
        };
        assert_eq!(
            CompiledAction::from(&action),
            CompiledAction::KeyCombo(KeyBinding {
                key_code: 1,
                modifiers: ModifierMask(0x120000)
            })
        );
    }

    #[test]
    fn modifier_names_come_out_in_display_order() {
        use crate::config::schema::KeyModifier::*;
        assert_eq!(
            modifier_names(ModifierMask(0x40000 | 0x80000 | 0x20000 | 0x100000)),
            vec![Control, Option, Shift, Command]
        );
    }

    #[derive(Default)]
    struct Labels(std::sync::Mutex<Vec<String>>);
    impl NoticeObserver for Labels {
        fn button_captured(&self, _: u8) {}
        fn key_captured(&self, capture: KeyCapture) {
            self.0.lock().unwrap().push(capture.key_label);
        }
        fn key_capture_cancelled(&self) {}
    }

    fn label_for(chars_len: u8) -> String {
        let labels = Labels::default();
        let a = u16::from(b'a');
        deliver(
            Notice::KeyCaptured(RawKeyCapture {
                key_code: 0x00,
                modifiers: ModifierMask(0),
                chars: [a, a + 1, a + 2, a + 3],
                chars_len,
            }),
            &labels,
        );
        labels.0.lock().unwrap().pop().expect("no key captured")
    }

    #[test]
    fn a_captured_label_uses_only_the_valid_chars() {
        assert_eq!(label_for(1), "A");
    }

    #[test]
    fn a_chars_len_past_four_is_clamped_to_the_four_chars_there_are() {
        assert_eq!(label_for(9), "ABCD");
    }

    #[test]
    fn every_modifier_name_maps_to_its_flag() {
        use crate::config::schema::KeyModifier::*;
        assert_eq!(modifier_mask(&[Command]), ModifierMask(0x100000));
        assert_eq!(modifier_mask(&[Shift]), ModifierMask(0x20000));
        assert_eq!(modifier_mask(&[Option]), ModifierMask(0x80000));
        assert_eq!(modifier_mask(&[Control]), ModifierMask(0x40000));
        assert_eq!(modifier_mask(&[]), ModifierMask(0));
    }
}
