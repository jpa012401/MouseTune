//! Zoom steps (Phase 6 design §3) through the
//! `MagnifyOutput` port: +1 zooms in, −1 zooms out.
//!
//! # Switched off until verified
//!
//! Two paths, chosen once when `MacMagnify` is built:
//! - **Gesture:** a began / changed / ended triple of magnify gesture events,
//!   what a trackpad pinch delivers. Used only where
//!   `platform::output_support().magnify` is true, that is on a macOS major
//!   version in `VERIFIED_ON`. That list is **empty** until the maintainer
//!   sees the probe (`magnify_probe`) zoom, so today no version uses it.
//! - **Keys (the fallback, active everywhere today):** ⌘= to zoom in, ⌘- to
//!   zoom out. It posts only the `=` / `-` key, with the Command flag on its
//!   down and up, and never a separate ⌘ key down or up: when the zoom key
//!   is bound to ⌘ the user is physically holding ⌘, and a synthetic ⌘ up
//!   could clear it from the session's flag state, so the next ticks would
//!   scroll instead of zooming.
//!
//! On the gesture path all three events are built before any is posted
//! (`choose`); if one won't build, that step posts the keys instead, so a
//! Began never goes out without its Ended.
//!
//! **No private symbols.** The gesture is a plain `CGEvent` made with
//! public CoreGraphics calls (`CGEventCreate`, `CGEventSetType`,
//! `CGEventSet{Integer,Double}ValueField`); only the type and field numbers
//! are undocumented. So nothing is `dlopen`ed or `dlsym`ed, MouseTune links
//! and launches on macOS 12–26 unchanged, and there is no `loads()` step in
//! the support gate (unlike `dock_swipe`).
//!
//! # Encoding (what the adapter and the probe send)
//!
//! | Fact | Value | Source |
//! |---|---|---|
//! | CGEvent type | `kCGSEventGesture` = **29** | [E] |
//! | Which gesture | field `kCGEventGestureHIDType` = **110** holds an `IOHIDEventType` | [E], [G] |
//! | The magnify subtype | `kIOHIDEventTypeZoom` = **8** in field 110 | [A] enum, [C] |
//! | Magnification | field `kCGEventGestureZoomValue` = **113**, a double field read on type-29/30/31 events whose field 110 is `kIOHIDEventTypeZoom` | [E], [G] |
//! | Phase | field `kCGEventGesturePhase` = **132**: `CGSGesturePhase` Began 1, Changed 2, Ended 4 (Cancelled 8, MayBegin 128 unused) | [E], [G] |
//! | Meaning of the value | `-[NSEvent magnification]`: "change in magnification … added to the current scaling of an item to get the new scale factor" | [H] |
//! | AppKit event type | `NSEventTypeMagnify` = 30 (`NSEventTypeGesture` = 29) | [H] |
//! | No begin/end-gesture bracket | `NSEventTypeBeginGesture`/`EndGesture` "no longer delivered. Use the phase property" | [H] |
//! | Building it | `CGEventCreate(NULL)` then setting the fields, as WebKit's event replayer does | [G] |
//! | Posting | `kCGSessionEventTap`, stamped with `MOUSETUNE_EVENT_TAG` | AGENTS.md footgun 3 |
//!
//! Checked on macOS 27.0.1 by CI tests that post nothing:
//! `a_built_change_reads_back_as_a_tagged_type_29_zoom_gesture` reads the
//! fields back, and `appkit_reads_the_built_triple_as_magnify_events` shows
//! `+[NSEvent eventWithCGEvent:]` decodes the triple as `NSEventTypeMagnify`
//! with phases Began, Changed, Ended (`NSEventPhase` 1, 4, 8) and the
//! magnification set. Two facts came from those runs, not from a source:
//! - field 113 is stored as a 32-bit float (0.1 reads back as
//!   0.100000001490116…);
//! - without field 110 = 8 AppKit doesn't decode a magnify event at all.
//!
//! How the adapter fills it in (`step_fields`): Began and Ended carry
//! magnification 0; the Changed carries `amount × STEP_SCALE`, so one wheel
//! tick is one complete pinch of ±10%.
//!
//! # The probe
//!
//! `magnify_probe` (ignored; the maintainer runs it) builds every event with
//! this module's `build` before posting any. Its default mode sends one
//! gesture each way (Began, 10 Changed of ±0.05, Ended), so a "worked"
//! result verifies the encoding. `MOUSETUNE_PROBE_MAGNIFY=ticks` sends 5
//! ticks each way through `choose`, exactly what `MacMagnify::step` posts.
//!
//! ## Unknown (no allowed source)
//!
//! - **Whether apps act on it when posted.** AppKit decodes it in-process;
//!   whether WindowServer routes a posted type-29 zoom to the window under
//!   the pointer, and whether Preview, Maps and Safari zoom, is what the
//!   probe shows.
//! - **Anything macOS 27 needs differently.** Dock swipes on 27 carry an
//!   attached `IOHIDEvent` (`dock_swipe`); whether magnify needs one too is
//!   unknown, and the HID zoom event's field layout isn't in any allowed
//!   header (IOHIDFamily-1915 defines no `kIOHIDEventTypeZoom` fields). Not
//!   sent.
//! - **`kCGEventGestureZoomDeltaX`/`Y` (139/140) and `kCGEventGestureMask`
//!   (133)** [E]: meaning unknown; left at 0.
//! - **Whether a whole began/changed/ended triple per tick feels right**, or
//!   apps want one gesture held open across ticks. `STEP_SCALE` is a guess.
//! - **macOS 12–26.** The fields are WebKit's from 2016, so the same
//!   encoding likely applies, but nothing was checked there.
//!
//! # Sources
//!
//! - [A] Apple open source, IOHIDFamily-1915.140.3, `IOHIDFamily/IOHIDEventTypes.h`.
//! - [C] WebKit, `Source/WebCore/PAL/pal/spi/ios/IOKitSPIIOS.h`.
//! - [E] WebKit, `Tools/TestRunnerShared/spi/CoreGraphicsTestSPI.h`.
//! - [G] WebKit, `Tools/TestRunnerShared/EventSerialization/mac/EventSerializerMac.mm`.
//! - [H] The macOS 27 SDK's `AppKit.framework/Headers/NSEvent.h`.
//!
//! Upstream Mac Mouse Fix was not consulted (spec §13).

use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGEvent, CGEventField, CGEventTapLocation, CGEventType};

use crate::{
    actions::keys::{KeyStroke, ModifierMask},
    actions::perform::EventPoster,
    input::{filter::MOUSETUNE_EVENT_TAG, intents::MagnifyOutput},
    platform::event_poster::MacEventPoster,
};

/// macOS major versions where the maintainer saw `magnify_probe` zoom
/// Preview or Maps in and then out, in its **default** mode (which proves the
/// encoding) and its `MOUSETUNE_PROBE_MAGNIFY=ticks` mode (which sends
/// exactly `MacMagnify`'s per-tick triples). Empty: nothing is confirmed,
/// so every version uses the ⌘= / ⌘- keys.
///
/// Flip checklist, when the maintainer reports "worked" on macOS 27:
/// 1. This line becomes `&[27]`.
/// 2. `platform::macos::tests::magnify_stays_off_everywhere_until_a_version_is_verified`
///    pins today's state and fails by design: make it "27 on, 26 off".
/// 3. Rename the probe `magnify_probe` → `magnify_zooms_preview`.
/// 4. Spec §8.5 Zoom note: replace the Status line with the result, the
///    build it was seen on, and any `STEP_SCALE` change; §3.4 Zoom row, 27
///    cell: "magnify gesture (verified 27.x)"; tick the zoom item in §12.
/// 5. README Phase 6 acceptance: zoom by gesture in Preview/Maps/Safari.
/// 6. `pnpm tauri dev`, then a hardware check with a real wheel.
pub const VERIFIED_ON: &[u32] = &[];

/// Magnification per step: `MagnifyOutput::step`'s ±1 times this. A tuning
/// knob, not a sourced value: 0.1 adds 10% to the current scale per wheel
/// tick (`-[NSEvent magnification]` is added to the scale factor).
pub const STEP_SCALE: f64 = 0.1;

const CG_GESTURE: u32 = 29;
const FIELD_GESTURE_HID_TYPE: u32 = 110;
const FIELD_ZOOM_VALUE: u32 = 113;
const FIELD_GESTURE_PHASE: u32 = 132;
const HID_TYPE_ZOOM: i64 = 8;
const PHASE_BEGAN: i64 = 1;
const PHASE_CHANGED: i64 = 2;
const PHASE_ENDED: i64 = 4;

/// Every value one magnify event carries besides its fixed type fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MagnifyFields {
    /// `CGSGesturePhase`: Began 1, Changed 2, Ended 4.
    pub phase: i64,
    pub magnification: f64,
}

/// The began / changed / ended triple for one step of `amount` (±1): only
/// the change carries magnification, `amount × STEP_SCALE`.
pub fn step_fields(amount: f64) -> [MagnifyFields; 3] {
    [
        MagnifyFields {
            phase: PHASE_BEGAN,
            magnification: 0.0,
        },
        MagnifyFields {
            phase: PHASE_CHANGED,
            magnification: amount * STEP_SCALE,
        },
        MagnifyFields {
            phase: PHASE_ENDED,
            magnification: 0.0,
        },
    ]
}

/// What one step posts.
#[derive(Debug, PartialEq)]
pub(crate) enum Step<B> {
    Gesture([B; 3]),
    Keys([KeyStroke; 2]),
}

/// The gesture triple if `gesture` is on and all three events build, else
/// the key fallback. All three are built before anything is posted, so a
/// Began never goes out without its Ended.
pub(crate) fn choose<B>(
    gesture: bool,
    amount: f64,
    build: impl FnMut(&MagnifyFields) -> Option<B>,
) -> Step<B> {
    if gesture {
        let mut build = build;
        let [began, changed, ended] = step_fields(amount);
        if let (Some(b), Some(c), Some(e)) = (build(&began), build(&changed), build(&ended)) {
            return Step::Gesture([b, c, e]);
        }
    }
    Step::Keys(zoom_key_strokes(amount))
}

/// One magnify event, built but not posted.
pub(crate) fn build(f: &MagnifyFields) -> Option<CFRetained<CGEvent>> {
    let event = CGEvent::new(None)?;
    let set = |field: u32, value: i64| {
        CGEvent::set_integer_value_field(Some(&event), CGEventField(field), value)
    };
    CGEvent::set_type(Some(&event), CGEventType(CG_GESTURE));
    set(FIELD_GESTURE_HID_TYPE, HID_TYPE_ZOOM);
    set(FIELD_GESTURE_PHASE, f.phase);
    CGEvent::set_double_value_field(
        Some(&event),
        CGEventField(FIELD_ZOOM_VALUE),
        f.magnification,
    );
    CGEvent::set_integer_value_field(
        Some(&event),
        CGEventField::EventSourceUserData,
        MOUSETUNE_EVENT_TAG,
    );
    Some(event)
}

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

/// `MagnifyOutput` on macOS. Runs from `run_intents` on `mt-input`, never
/// inside a tap callout. Never panics: a gesture that won't build becomes
/// that step's key fallback.
pub struct MacMagnify {
    /// Chosen once, at construction: the gesture path, or the keys only.
    gesture: bool,
}

impl Default for MacMagnify {
    fn default() -> Self {
        Self::new()
    }
}

impl MacMagnify {
    /// The gesture path exactly where `output_support().magnify` is true
    /// (this macOS is in `VERIFIED_ON`), else ⌘= / ⌘- for every step.
    pub fn new() -> Self {
        Self {
            gesture: super::output_support().magnify,
        }
    }
}

impl MagnifyOutput for MacMagnify {
    fn step(&mut self, amount: f64) {
        match choose(self.gesture, amount, build) {
            Step::Gesture(events) => {
                for event in &events {
                    CGEvent::post(CGEventTapLocation::SessionEventTap, Some(event));
                }
            }
            Step::Keys(strokes) => {
                let _ = MacEventPoster.post_keys(&strokes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2_app_kit::{NSEvent, NSEventPhase, NSEventType};

    use super::*;

    const fn mf(phase: i64, magnification: f64) -> MagnifyFields {
        MagnifyFields {
            phase,
            magnification,
        }
    }

    #[test]
    fn a_step_in_is_began_then_a_tenth_then_ended() {
        assert_eq!(step_fields(1.0), [mf(1, 0.0), mf(2, 0.1), mf(4, 0.0)]);
    }

    #[test]
    fn a_step_out_changes_by_minus_a_tenth() {
        assert_eq!(step_fields(-1.0), [mf(1, 0.0), mf(2, -0.1), mf(4, 0.0)]);
    }

    /// A builder that fails for every field set in `broken`.
    fn builder(
        broken: &[MagnifyFields],
    ) -> impl FnMut(&MagnifyFields) -> Option<MagnifyFields> + '_ {
        move |f| (!broken.contains(f)).then_some(*f)
    }

    #[test]
    fn with_the_gesture_on_a_step_posts_the_whole_triple() {
        assert_eq!(
            choose(true, 1.0, builder(&[])),
            Step::Gesture([mf(1, 0.0), mf(2, 0.1), mf(4, 0.0)])
        );
    }

    #[test]
    fn with_the_gesture_off_a_step_posts_keys_and_builds_nothing() {
        let step = choose(false, -1.0, |_: &MagnifyFields| -> Option<()> {
            panic!("built a gesture while it is off")
        });
        assert_eq!(step, Step::Keys([stroke(0x1B, true), stroke(0x1B, false)]));
    }

    #[test]
    fn a_step_whose_ended_wont_build_falls_back_to_keys_without_a_began() {
        assert_eq!(
            choose(true, 1.0, builder(&[mf(4, 0.0)])),
            Step::Keys([stroke(0x18, true), stroke(0x18, false)])
        );
        assert_eq!(
            choose(true, 1.0, builder(&[mf(2, 0.1)])),
            Step::Keys([stroke(0x18, true), stroke(0x18, false)])
        );
    }

    /// Public CoreGraphics getters only; posts nothing.
    #[test]
    fn a_built_change_reads_back_as_a_tagged_type_29_zoom_gesture() {
        let event = build(&mf(2, 0.1)).expect("builds");
        let int = |n: u32| CGEvent::integer_value_field(Some(&event), CGEventField(n));
        assert_eq!(CGEvent::r#type(Some(&event)), CGEventType(29));
        assert_eq!(int(110), 8, "kCGEventGestureHIDType = kIOHIDEventTypeZoom");
        assert_eq!(int(132), 2, "kCGEventGesturePhase = Changed");
        assert_eq!(
            CGEvent::double_value_field(Some(&event), CGEventField(113)),
            0.100_000_001_490_116_12,
            "kCGEventGestureZoomValue"
        );
        assert_eq!(
            CGEvent::integer_value_field(Some(&event), CGEventField::EventSourceUserData),
            MOUSETUNE_EVENT_TAG
        );
    }

    /// AppKit's own decoding (public API, posts nothing): what an app's
    /// `magnifyWithEvent:` would receive.
    #[test]
    fn appkit_reads_the_built_triple_as_magnify_events() {
        let read = |f: MagnifyFields| {
            let cg = build(&f).expect("builds");
            let ns = NSEvent::eventWithCGEvent(&cg).expect("back to an NSEvent");
            (ns.r#type(), ns.phase(), ns.magnification())
        };
        assert_eq!(
            read(mf(1, 0.0)),
            (NSEventType::Magnify, NSEventPhase::Began, 0.0)
        );
        assert_eq!(
            read(mf(2, -0.1)),
            (
                NSEventType::Magnify,
                NSEventPhase::Changed,
                -0.100_000_001_490_116_12
            )
        );
        assert_eq!(
            read(mf(4, 0.0)),
            (NSEventType::Magnify, NSEventPhase::Ended, 0.0)
        );
    }

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

/// The maintainer's hardware probe. Every event comes from
/// the adapter's own `build` (and, in the ticks mode, `choose`), and all of
/// them are built before any is posted.
#[cfg(test)]
mod probe {
    use std::{thread::sleep, time::Duration};

    use super::*;

    const CHANGES: u32 = 10;
    const CHANGE: f64 = 0.05;
    const GAP: Duration = Duration::from_millis(30);
    const TICKS: u32 = 5;
    const TICK_GAP: Duration = Duration::from_millis(200);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Mode {
        /// One gesture per direction: Began, 10 Changed of ±0.05, Ended.
        Gesture,
        /// What `MacMagnify::step` sends per wheel tick, 5 ticks each way.
        Ticks,
    }

    impl Mode {
        fn from_env() -> Self {
            match std::env::var("MOUSETUNE_PROBE_MAGNIFY").as_deref() {
                Ok("ticks") => Self::Ticks,
                _ => Self::Gesture,
            }
        }
    }

    struct Planned {
        wait: Duration,
        event: CFRetained<CGEvent>,
        line: String,
    }

    fn describe(f: &MagnifyFields) -> String {
        let phase = match f.phase {
            PHASE_BEGAN => "Began",
            PHASE_CHANGED => "Changed",
            PHASE_ENDED => "Ended",
            _ => "?",
        };
        format!("{phase} magnification {:+.3}", f.magnification)
    }

    fn planned(wait: Duration, f: &MagnifyFields, event: CFRetained<CGEvent>) -> Planned {
        Planned {
            wait,
            event,
            line: describe(f),
        }
    }

    fn one_gesture(sign: f64) -> Result<Vec<Planned>, String> {
        let mut steps = vec![(
            Duration::ZERO,
            MagnifyFields {
                phase: PHASE_BEGAN,
                magnification: 0.0,
            },
        )];
        for _ in 0..CHANGES {
            steps.push((
                GAP,
                MagnifyFields {
                    phase: PHASE_CHANGED,
                    magnification: sign * CHANGE,
                },
            ));
        }
        steps.push((
            GAP,
            MagnifyFields {
                phase: PHASE_ENDED,
                magnification: 0.0,
            },
        ));
        steps
            .iter()
            .map(|(wait, f)| {
                let event = build(f).ok_or_else(|| format!("{} didn't build", describe(f)))?;
                Ok(planned(*wait, f, event))
            })
            .collect()
    }

    fn adapter_ticks(sign: f64) -> Result<Vec<Planned>, String> {
        let mut out = Vec::new();
        for _ in 0..TICKS {
            let built = |f: &MagnifyFields| build(f).map(|event| (*f, event));
            let Step::Gesture(triple) = choose(true, sign, built) else {
                return Err("a tick's triple didn't build (the adapter would post keys)".into());
            };
            for (i, (f, event)) in triple.into_iter().enumerate() {
                let wait = if i == 0 { TICK_GAP } else { Duration::ZERO };
                out.push(planned(wait, &f, event));
            }
        }
        Ok(out)
    }

    /// Run by the maintainer only:
    /// `cargo test magnify_probe -- --ignored --nocapture`, then optionally
    /// `MOUSETUNE_PROBE_MAGNIFY=ticks cargo test magnify_probe -- --ignored --nocapture`.
    /// Posts real events. Watch the screen.
    #[test]
    #[ignore = "posts real magnify events; the maintainer runs and watches it"]
    fn magnify_probe() {
        if !crate::platform::permissions::is_accessibility_trusted() {
            println!(
                "WARNING: this terminal isn't trusted for Accessibility; macOS will drop the events."
            );
        }
        let mode = Mode::from_env();
        let plan = |sign| match mode {
            Mode::Gesture => one_gesture(sign),
            Mode::Ticks => adapter_ticks(sign),
        };
        println!(
            "Mode: {}.",
            match mode {
                Mode::Gesture => "gesture (Began, 10 x Changed of 0.05, Ended, each way)",
                Mode::Ticks => "ticks (5 wheel ticks each way, exactly what MacMagnify sends)",
            }
        );
        let (zoom_in, zoom_out) = match (plan(1.0), plan(-1.0)) {
            (Ok(i), Ok(o)) => (i, o),
            (Err(why), _) | (_, Err(why)) => {
                println!("Posted nothing, because {why}.");
                return;
            }
        };
        println!("Keep the pointer over the window.");
        println!("In 3 s: zoom IN on the frontmost Preview or Maps window, then OUT.");
        sleep(Duration::from_secs(3));
        for (label, events) in [("IN", zoom_in), ("OUT", zoom_out)] {
            println!("Zoom {label}:");
            for p in events {
                sleep(p.wait);
                CGEvent::post(CGEventTapLocation::SessionEventTap, Some(&p.event));
                println!("  {}", p.line);
            }
            sleep(Duration::from_secs(1));
        }
    }
}
