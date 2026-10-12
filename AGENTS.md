# MouseTune

MouseTune is a macOS menu bar app that makes ordinary mice feel like a trackpad: smooth scrolling, remappable extra buttons, and drag gestures for Spaces and Mission Control. It runs unattended in the background on someone's everyday Mac, **inside the path of every mouse event**. A mistake doesn't show up as an error message. It shows up as a frozen cursor, a broken trackpad, or scrolling that silently stops working.

It is a clean-room reimplementation inspired by [Mac Mouse Fix](https://github.com/noah-nuebling/mac-mouse-fix). Any upstream checkout is **read-only reference: never copy code from it** (spec §13).

It ships as a Tauri v2 app (Rust core, React settings window) for **macOS 12 Monterey through macOS 27**, on Apple Silicon and Intel. What it does and why is in [the spec](macos_mouse_utility_technical_specification.md), §1.

## How to use this file

- This file holds **standards and pointers**, not history. History lives in `git log`. The active plan is the newest file in `docs/superpowers/plans/`. Session progress is that plan's ledger at `.superpowers/sdd/<plan>/progress.md` (gitignored).
- **Read only what your change touches.** See "Before you change an area, read".
- **Run only the checks your change needs.** See "Verifying".
- An explicit instruction from the maintainer overrides anything here except the **Non-negotiables**. If a rule conflicts with the task, say so and ask; don't quietly work around it.
- When the code and this file disagree, the code has won. Fix this file in the same change.

## Four things this project can't trade away

### 1. It sits in front of every mouse event on the machine
A slow event-tap callback makes macOS disable the tap (`kCGEventTapDisabledByTimeout`), so scrolling or buttons silently stop. A panic that unwinds into CoreGraphics aborts the process. **Tap callbacks do O(1) work: no allocation, no locks, no I/O, no blocking channel sends. They run inside `catch_unwind`, and when in doubt they pass the event through unchanged.** Spec §4.5.

### 2. Never break the devices it isn't for
The v1 draft of the spec swallowed *every* scroll event, which would have broken trackpad and Magic Mouse scrolling for everyone. **MouseTune only modifies discrete wheel ticks and button events from tracked mice. Everything else passes through untouched.** The exact rules are in spec §4.2 and §4.3.

### 3. It depends on private macOS APIs that will change
Dock-swipe events, CGS symbolic hotkeys and CGEvent field 87 are undocumented. The dock-swipe format already changed in macOS 27. **Every OS call sits behind a port (a trait) with a humble adapter, version-gated code branches on `platform::os_version`, and each private-API feature keeps a documented fallback.** Spec §3.4 and §6.1.

### 4. Someone will read and extend this alone
This is a personal project. The maintainer will come back to it after months away. **Plain names, small modules and an obvious design beat a clever one.** A test that pins behaviour is worth more than a paragraph explaining it.

## A note from the maintainer

> *Draft written by the agent from the maintainer's stated preferences. Maintainer: rewrite this in your own voice.*

I want this built properly: test first, SOLID, DRY, and small reviewed steps through subagents. I don't want it built fast and fixed later. Prefer the smallest design that makes the correct behaviour obvious. If you can't say which requirement a pattern serves, don't add it.

This is a mouse utility for my own Macs, not a platform. No accounts, no telemetry, no licensing system. Mac Mouse Fix is the reference for behaviour and for which macOS tricks work, not a source to translate.

## Glossary

- **Tap**: a CGEventTap. Four exist: Buttons, Scroll, Flags (modifier keys) and Keys (only while the recorder runs). They are switched on and off as a set (`TapSet`), never one by one from feature code, except that the key recorder releases its own Keys tap from inside its callback (`TapManager::release`).
- **Tracked mouse**: a connected HID mouse that isn't excluded. Only tracked mice turn taps on.
- **Excluded device**: Apple vendor IDs `0x05AC` or `0x004C`, or a name containing "trackpad". These already behave like trackpads; we leave them alone.
- **Sender ID**: CGEvent field 87, the IORegistry ID of the HID service that produced an event. It's how an event is matched to a device.
- **Pass through**: return the original event unmodified. This is the default answer to any doubt.
- **Degraded**: a tap callback panicked and was caught. The engine keeps running, and the UI says so.
- **Capture**: the next mouse-button press is reported to the UI instead of acting. **Recorder**: the temporary active Keys tap that records a keyboard shortcut.
- **Trusted**: the app has Accessibility permission (`AXIsProcessTrusted`). Never say "has permissions" loosely; this is the only permission that matters.
- **Job**: an action or plain click the input engine hands to `mt-exec`.
- **`mt-exec`**: the executor thread that performs actions off the hot path, over a bounded 64-slot queue. A full queue drops the job and counts it.
- **Port / adapter / humble object**: a port is a trait the core depends on (`Accessibility`, `TapController`). An adapter implements it with real OS calls and contains no decisions.
- **System Settings / System Preferences**: macOS 13+ / macOS 12 naming. User-facing copy must pick the right one (`src/lib/os.ts`).
- **MouseTune**: the product. Never "MouseFix" or "Mac Mouse Fix" in code, UI or bundle IDs. **Upstream** means Mac Mouse Fix.
- **Bundle ID** `com.johnpaulalabe.mousetune`: one source of truth, `src-tauri/tauri.conf.json` → `identifier`. The config, log and TCC paths derive from it.

## The ways to hurt yourself

1. **Doing work in a tap callback.** Logging, allocating, locking or calling IOKit there causes timeouts that disable input. Copy fields out, decide in O(1), and push slow work to `mt-exec`.
2. **Swallowing an event you don't own.** Returning `NULL` for a trackpad or tablet event breaks that device for the whole Mac. Swallowing is only allowed after the scroll filter or button routing has said the event is ours.
3. **Posting synthetic events where your own tap sees them.** Post at `kCGSessionEventTap` and stamp `MOUSETUNE_EVENT_TAG`. Posting at the HID tap feeds your output back into your input.
4. **Calling `platform::*` from feature code.** It bypasses the port, so the logic becomes untestable and is wired to one macOS version. Go through the injected service.
5. **Hand-editing `src/bindings.ts`.** It's regenerated by `cargo test`, so your edit vanishes and the types drift. Change the Rust type instead.
6. **"Fixing" Accessibility in code.** In `pnpm tauri dev` the grant belongs to the terminal or editor that launched the app. Ad-hoc signed bundles lose the grant on every rebuild. These are environment facts (README); don't add workarounds.
7. **Running `tccutil reset`.** It changes the maintainer's system permission state. The in-app button does this deliberately; you must not.
8. **Killing processes by pattern.** `pkill -f vite` or `pkill -f mousetune` also kills the maintainer's own dev servers and sessions. Stop only processes you started, by the PID or task ID you hold.
9. **Copying upstream code.** Mac Mouse Fix's licence puts conditions on derived code (spec §13). Re-derive behaviour; cite facts, not source.
10. **Treating "it launched" as "it works".** Without Accessibility, taps fail to create and the app still starts cleanly. A clean launch proves nothing about input handling.

## Workflow

Order: **brainstorm → spec change → plan → subagent execution → review → finish branch**. The maintainer approves the spec change and the plan before any code is written.

| Change | Gates |
|---|---|
| **Feature-sized**: a new module under `input/` or `platform/`, a new IPC command or event, a new user-facing screen or string set, a new private API, a dependency change, or anything touching more than one of {Rust core, adapters, IPC, UI} | All. No code before an approved plan in `docs/superpowers/plans/`. Execute with `superpowers:subagent-driven-development`. |
| **Small**: a fix or tweak inside one module with no new public API | Plan in the conversation; can be done inline. **Still test first.** |
| **Docs or config only** | Review the diff; run the matching verification row. |

When unsure which row applies, ask.

**Skills:** `superpowers:brainstorming`, `writing-plans`, `subagent-driven-development`, `test-driven-development`, `requesting-code-review`, `finishing-a-development-branch`. If a skill conflicts with this file, this file wins. Implementer subagents never dispatch their own reviewers.

### Hit every surface (feature-sized changes)

Report which of these applied:
- **Core logic** in `input/` or a use-case module, with fakes in `test_support.rs`.
- **Port and adapter**: a new trait for any new OS call, plus its humble adapter in `platform/macos/`.
- **macOS 12–27**: version gates and fallbacks (spec §3.4).
- **IPC**: command or DTO in `ipc.rs`, event in `events.rs`, regenerated `src/bindings.ts`.
- **UI**: Zustand store, route or component, copy (System Settings vs System Preferences).
- **Tray**, if the feature has a status or toggle.
- **Tests** on both sides, including the failure path.
- **Spec**: tick the §12 roadmap and amend any section the code proved wrong. **README**, if a user-visible step changed.

### Working here

- **Ask before:** pushing, merging into `main`, force-pushing, deleting branches or files you didn't create, running `tccutil`, opening or posting to anything outside this machine, or changing signing or notarization settings.
- **Commit** on a feature branch, never feature work on `main`. Don't open PRs unless asked.
- **Scratch files** go in the session scratchpad, not the repo.
- **Upstream reference checkouts** live outside the repo, never inside it.

## Verifying

Run the checks for what you changed, and nothing more.

| You changed | Run |
|---|---|
| Rust core (`src-tauri/src/**` except `platform/`) | `cd src-tauri && cargo test <module_path>` while iterating, then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` |
| `platform/macos/*` adapters | The row above, plus the adapter's `#[ignore]` hardware test if it has one (the first arrives in Phase 2 Task 7): `cargo test <name> -- --ignored --nocapture`. Paste its output. |
| A type crossing IPC (`ipc.rs`, `events.rs`, anything deriving `specta::Type`) | `cd src-tauri && cargo test`, then `git diff --exit-code src/bindings.ts` (commit the regenerated file), then `pnpm typecheck` |
| Frontend (`src/**`) | `pnpm typecheck && pnpm test` |
| `vite.config.ts`, CSS, Tailwind | `pnpm build` |
| `tauri.conf.json`, `Info.plist`, capabilities, tray, window | `pnpm tauri dev` for ~15 s; check the log for `MouseTune started` and no panic. Stop it by its task ID. |

Crate: `mousetune` (lib `mousetune_lib`), in `src-tauri/`. npm package: `mousetune` (pnpm), at the repo root.

### Do not run these unless the maintainer asks

- **`pnpm tauri build`** (any target): slow, triggers signing, and has never been run on this project.
- **`tccutil reset …`**: changes system permission state.
- **`cargo tauri icon …`**: overwrites `src-tauri/icons/`.
- **Anything that clicks "Open System Settings" or calls `request_accessibility`**: it shows a system prompt to the maintainer.

### What counts as verified

- Tests were written first and **seen to fail** for the right reason. For a retrofit, prove each test can fail by mutating the production code, then restore it.
- Tests check observable behaviour (outputs, published snapshots, rendered text), not wiring or constants. Expected values are hand-derived literals, never computed with the code under test, or the test can't fail.
- A bug fix starts with a failing test that reproduces the bug. That proves the fix and keeps the bug from coming back.
- Tap handling, device add/remove, permission changes and IPC error results need a **failure-path test** as well as the happy path.
- `cargo clippy` green means it type-checks and lints. It doesn't mean it runs. `pnpm tauri dev` launching doesn't mean input works (footgun 10).
- **Say plainly what you couldn't verify**: real hardware, system prompts, Monterey, Intel, CI that hasn't run.

## Non-negotiables

| Rule | Check | State |
|---|---|---|
| Rust formatted and lint-clean | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` in CI | enforced |
| Every `unsafe` block has a `// SAFETY:` comment | `[lints.clippy] undocumented_unsafe_blocks = "deny"` in `src-tauri/Cargo.toml` | enforced |
| `src/bindings.ts` matches the Rust types | CI runs `cargo test`, then `git diff --exit-code src/bindings.ts` | enforced |
| Frontend type-checks and its tests pass | `pnpm typecheck`, `pnpm test` in CI | enforced |
| Minimum macOS 12.0 | `bundle.macOS.minimumSystemVersion` in `tauri.conf.json` | enforced |
| Frontend JS runs on Safari 15 | Vite `build.target: "safari15"` | enforced for JS syntax; **CSS features are honour-system** (Tailwind 3.4, no `:has()`) |
| Tests first | review | honour-system |
| No OS call outside `platform/` or a `System*` adapter | review | honour-system |
| Tap callbacks: O(1), guarded, pass through on doubt | review; `input::guard::guarded` once Phase 2 lands | honour-system |
| No upstream code copied | review | honour-system |
| Commit author `alabejohnpaul@gmail.com` | repo-local `git config` | enforced, not scripted |

Argue with these by changing the check, not by working around it.

## Architecture

One process. The Tauri main thread owns windows, tray and IPC. `mt-permissions` polls Accessibility trust. `mt-input` (Phase 2) runs a CFRunLoop holding the event taps, the IOHID device monitor and the watchdog timer, and owns all input state, so the hot path takes no locks. State flows out to the UI as typed events and to `get_status` through an `ArcSwap` snapshot. Spec §2.

| Home | Holds | May import |
|---|---|---|
| `src-tauri/src/lib.rs` | Composition root: builds adapters, injects them, registers commands and events | everything |
| `ipc.rs`, `events.rs`, `tray.rs`, `window.rs` | Tauri adapters: thin commands, DTOs, event emitters, observers | core modules, `tauri` |
| `accessibility.rs`, `permission_monitor.rs`, `input/*` | Core: ports, use cases, state machines | `std`, `serde`/`specta` derives, each other. **Not `tauri`.** `platform::*` only inside a `System*` adapter impl |
| `platform/macos/*` | Humble FFI adapters | `objc2*` crates, `std`, and the port types they implement. **Not `tauri`** |
| `test_support.rs` | `#[cfg(test)]` fakes and fixtures shared by tests | core types |
| `src/bindings.ts` | Generated IPC client and types | — (never edit) |
| `src/store/` | Zustand mirrors of backend state, event subscriptions | `bindings` |
| `src/routes/`, `src/components/` | Screens and UI pieces | `store`, `bindings`, `lib` |
| `src/lib/` | Pure TS helpers shared by components | `bindings` types only |

### Patterns, and what each is for

- **Ports and adapters**: each OS boundary changes across macOS releases (invariant 3) and can't run in CI. The trait keeps decisions testable.
- **Observer** (`PermissionObserver`, `EngineObserver`): new reactions plug in without editing the emitter (open/closed).
- **Thin Tauri commands**: commands are untestable glue. Logic lives in a use-case function with a test.
- **Snapshot plus dedup publish**: the UI sees consistent state, and events fire only on real change.
- **`OnceCell`/`Rc<RefCell>` context for tap callbacks**: C callbacks need a stable pointer, and all mutation stays on `mt-input`.

If you can't name the requirement a pattern serves, don't add it. **DRY is about knowledge, not similar-looking text:** each fact (bundle ID, tap masks, exclusion rules, settings app name) is defined once.

## Current state

- **Implemented on phase-6-modifiers-and-drag, not yet merged:** Phase 6 (drag tap and router drags, intent ring, pointer freeze with release guarantees, hold-through, button + scroll effects, Discrete / two-finger / dock-swipe drags, Buttons UI). Dock swipe and magnify are built but switched off (`VERIFIED_ON` empty), so Spaces/Mission Control run as Discrete and zoom posts ⌘= / ⌘-. **Pending manual hardware acceptance** (README).
- **Merged on `main`:** Phase 0 (scaffold), Phase 1 (Accessibility onboarding, permission monitor, tray status, settings shell), Phase 2 (input thread, event taps, HID mouse tracking, General page), Phase 3 (config store with two kill switches, typed config commands and event, tray switches, recovery notice), Phase 4a (click cycle, router, `mt-exec` executor with `Navigate` and symbolic hotkeys), Phase 4b (capture mode, key recorder, editable Buttons screen, the full hotkey table, `KeyCombo`), Phase 5a (scroll config section, acceleration/curve/animator core, CVDisplayLink frame driver with timer fallback, scroll poster, in-place inversion, tray toggles), Phase 5b (display-link backstop, scroll modifier keys, precise setting, swipe swift, Scroll screen with curve plot) and Phase 4c (mouse-click and media-key remaps, nav_overrides, configurable timing). Phases 2–5b and 4c are **pending manual hardware acceptance** (README). The scroll tap swallows plain mouse-wheel ticks while smoothing is on; the button tap swallows routed events.
- **Next:** Phase 7 (launch at login, release build, signing and notarization).
- **Known gaps:**
  - Config holds the two kill switches, `buttons.remaps` (editable in the UI since Phase 4b), timing (sliders since Phase 4c), nav_overrides (file-only) and the scroll section (editable on the Scroll screen since Phase 5b); later phases add their sections.
  - The Accessibility commands still return `Result<(), String>`; config commands use the typed `ConfigError`.
  - Parked UI edge cases (bridge-failure-only): a failed Dismiss or config load is only reported on the General page; brief stale switch if a tray change races the initial load.
  - Any config save (even a remap edit) closes a scroll gesture in flight (clean Ended).
  - Any config save releases a held hold-through and ends a drag in flight (refresh).
  - Deferred: a shutdown join timeout (also for `mt-exec` on quit; the 7 px drag cancel waits for Phase 6), a tap-timeout counter, a degraded indicator in the tray, and moving `spawn` out of `input/`.
- **Never exercised for real:** the Accessibility prompt and settings-pane flow; the tray icon rendering; a bundled or signed build; CI on GitHub (no remote yet); macOS 12–26; Intel; an external mouse through the HID adapter; tap teardown on quit with real taps; tray switch checkmarks; a kill switch turning a real tap off; the watchdog's tap rebuild (`revive`); the CGS install path (unreachable binding); posting key combos and clicks; the click timers on real input; the B3/B4/B5 defaults on a real mouse; button-tap swallowing with real taps; the Keys tap under Accessibility; button capture on hardware; whether macOS asks for Input Monitoring; the new hotkeys' CGS IDs and default-shortcut fallbacks; recording ⌘Q / ⌘Tab / ⌘Space; smooth scrolling on real hardware (the animator's feel, Safari rubber-banding, momentum); CVDisplayLink on macOS 12–26 and with mixed 60/120 Hz displays; the 120 Hz timer fallback outside tests; in-place inversion on a real wheel; scroll modifier keys and swipe swift on real hardware; the display-link backstop with a real unplugged display; mouse-click remaps and their click state; media keys (system-defined NSEvent created on mt-exec); nav_overrides edits; configurable timing on a real mouse; drags, pointer freeze and its release paths, hold-through, App Switcher and zoom on real hardware; dock swipe and magnify on macOS 12–26; the AppKit magnify read-back test pins macOS 27.0.1 values and CI hasn't run it; the intents run-loop source firing after a real callout. *Exercised:* reading hotkey 32 (enabled, Ctrl+Up) and the frontmost bundle ID on macOS 27.

## Settled decisions and open questions

**Settled.** The spec §0 revision table holds the full list. The ones most often re-argued:
- **Single process**, no helper LaunchAgent (§2.1). Reverses if crash isolation proves necessary (§12, Phase 8).
- **Tauri 2.x, not the 3.0 alpha.** `specta`/`tauri-specta` are exact-pinned at `=2.0.0-rc.25`, as their docs require.
- **Tailwind 3.4, not 4**: v4 needs Safari 16.4 and Monterey ships Safari 15 (§3.4).
- **Name MouseTune, bundle ID `com.johnpaulalabe.mousetune`, macOS 12–27.**
- **Tray toggles moved to Phase 3**: they need the config store.
- **Config v1 holds the kill switches and button remaps**, and grows a section per phase; the loader deep-merges the file over the defaults so it grows additively, and IPC types stay strict.
- **Phase 4 is split into 4a (button engine) and 4b (capture mode, editor); 4c holds mouse-click remaps, media keys, `nav_overrides` and timing.** **B3 click default = Mission Control.**
- **Phase 4c: a MouseClick remap is a full click, not hold-through (that's Phase 6). nav_overrides is file-only and merges over the Apple defaults; mt-exec reads it through an ArcSwap, never the job queue. Timing is 150–600 ms, clamped on compile.**
- **Phase 5 is split into 5a (engine, tray toggles) and 5b (modifiers, Scroll screen); Cmd + scroll = zoom moves to Phase 6.** **Frames come from CVDisplayLink on every macOS version**, with a 120 Hz timer fallback, because NSScreen.displayLink is main-thread AppKit. **In 5a a tick with a modifier key held isn't smoothed.** Inversion is relative to the delivered deltas (Phase 5a design).
- **Scroll modifiers (Phase 5b): one key per effect, each key at most once; defaults Shift = horizontal, Option = precise, swift unbound, so ⌃ + scroll stays with Accessibility Zoom. Unbound keys pass through. Screen-size scaling was dropped.**
- **The engine owns `CompiledRemaps`**, delivered by `Control::SetRemaps`; no `ArcSwap`.
- **Edits save immediately through `set_config`**; Rust validates remaps (`config/validate.rs`). No draft state.
- **Config actions compile to a `Copy` `CompiledAction`**, so the tap callback never touches heap data.
- **Hot-path results (captures) leave as `Job::Notice`** on the bounded queue to `mt-exec`.
- **Phase 6:** tap callouts queue Copy intents (`input::intents`) and never post; the pointer is unfrozen and held buttons/keys are released on every refresh, tap disable, shutdown and caught panic; private outputs (dock swipe, magnify) are used only on macOS versions their spike verified, otherwise their fallbacks (Discrete; ⌘= / ⌘-). B3 hold is no longer a default.
- Click-cycle, suppression and hotkey-fallback details: `input/{click_cycle,router}.rs` and `actions/` docs and tests.
- **Phase 2 passed every event through**; since Phase 3 the tap demand follows the config. **The watchdog runs on `mt-input`**, not the main thread. **One `input-status-changed` event** replaces `devices-changed`/`status-changed` (Phase 2 plan).

**Open, for the maintainer. Do not decide these in code:**
- Personal use or public distribution. This decides notarization and the licence review.
- Final licence. `LICENSE` is MIT provisionally.
- Trademark and App Store name check for "MouseTune".

Spec amendments go in the commit that proves the spec wrong.

## Before you change an area, read

| Area | Spec | Code / notes |
|---|---|---|
| Permissions, onboarding | §3.1 | `accessibility.rs`, `permission_monitor.rs`, `src/routes/Onboarding.tsx` |
| Tray, window, agent behaviour | §2.1, §3.2 | `tray.rs`, `window.rs` |
| macOS version differences | §3.4 | `platform/macos/os_version.rs` |
| Event taps, filtering, devices, threading | §4 | `input/`, Phase 2 plan |
| Buttons and gestures | §5 | `input/{remaps,click_cycle,router,capture,key_recorder}.rs`, `config/validate.rs`, `actions/`, `actions/nav.rs` |
| Drag, dock swipe | §6 | `input/{router,intents,drag}.rs`, `platform/macos/{pointer,dock_swipe,magnify}.rs`, Phase 6 design |
| Actions, symbolic hotkeys | §7 | `actions/`, `platform/macos/{symbolic_hotkeys,frontmost_app}.rs`, `platform/macos/event_poster.rs` (clicks, system keys) |
| Smooth scrolling | §8 | `input/scroll/` (incl. `modifiers.rs`, `preview.rs`), `platform/macos/{display_link,scroll_poster}.rs`, `src/routes/Scroll.tsx`, Phase 5a and 5b designs |
| Config schema | §9 | `config/` |
| IPC contract | §10 | `ipc.rs`, `events.rs`, `src/bindings.ts` |
| Settings UI | §11 | `src/` (incl. `src/routes/Scroll.tsx`) |
| Licensing, upstream | §13 | — |

**Sources of truth, in rank order:** code and tests → spec → active plan → this file. Where code and a document disagree, the code wins and the document is wrong: fix the document.

## Where code lives

```
.
├─ AGENTS.md, CLAUDE.md            # this file; CLAUDE.md just imports it
├─ macos_mouse_utility_technical_specification.md
├─ docs/superpowers/plans/         # approved implementation plans
├─ .superpowers/                   # SDD ledgers and briefs (gitignored, scratch)
├─ assets/                         # icon sources (SVG)
├─ src/                            # React settings UI
│  ├─ bindings.ts                  # GENERATED by `cargo test`; never edit
│  ├─ store/  routes/  components/  lib/
│  └─ test/                        # Vitest setup and shared helpers
├─ src-tauri/
│  ├─ src/                         # Rust: see the Architecture homes table
│  ├─ icons/                       # generated from assets/; tray-template@2x.png is hand-made
│  ├─ capabilities/  tauri.conf.json  Info.plist  Entitlements.plist
│  ├─ gen/                         # GENERATED by Tauri; gitignored
│  └─ target/                      # build output; gitignored
├─ dist/                           # Vite output; gitignored
└─ .github/workflows/ci.yml
```

## Platforms and toolchain

- **Target:** macOS 12.0–27, universal (`aarch64-apple-darwin`, `x86_64-apple-darwin`). **Dev:** macOS 27 on Apple Silicon.
- **Confirmed:** Rust stable (edition 2024), Node 22, pnpm 9.12 (`packageManager` field), Tauri 2.12, TypeScript 7, Vite 8, React 19, React Router 8, Vitest 5, Tailwind 3.4. Exact versions are in `Cargo.lock` and `pnpm-lock.yaml`.
- **CI** (`.github/workflows/ci.yml`, `macos-latest`): typecheck, Vitest, Vite build, rustfmt, clippy, `cargo test`, bindings drift. It hasn't run yet because there's no remote.
- **Traps:**
  - Rust won't compile without `dist/` (`generate_context!` checks `frontendDist`). On a fresh clone run `pnpm exec vite build` (or `mkdir dist`) before the first `cargo test`.
  - The UI runs in WebKit, not Chrome. On Monterey that's Safari 15.
  - `cargo test` writes `src/bindings.ts`.

## Dev commands

```sh
pnpm install                                   # first time / after lockfile change
pnpm exec vite build                           # needed once before first cargo build (creates dist/)
pnpm tauri dev                                 # run the app (needs Accessibility for the launching app)

pnpm typecheck                                 # tsc for app and config files
pnpm test                                      # Vitest, once
pnpm build                                     # typecheck + production bundle

cd src-tauri
cargo test                                     # Rust tests; regenerates ../src/bindings.ts
cargo test <path>                              # one module, e.g. `cargo test accessibility`
cargo fmt                                      # format
cargo fmt --check                              # CI form
cargo clippy --all-targets -- -D warnings      # lint, as CI runs it

git diff --exit-code -- src/bindings.ts        # MUST be clean after `cargo test`, or commit the regenerated file
```

## Test data and fixtures

- **Rust:** `src-tauri/src/test_support.rs` holds the shared fakes and fixtures. Fakes share state across clones, so a test keeps a handle after moving one into the code under test. Put new shared doubles here, not in production types.
- **Frontend:** IPC is mocked with `mockIPC(…, { shouldMockEvents: true })`. Fixtures must mirror the full `Status` shape from `bindings.ts`. The helpers (`mockBackend`, `status`, `inputStatus`, `device`) live in `src/test/backend.ts`.
- Device fixtures use real vendor IDs, but their sender IDs are invented. Real sender-ID layouts are confirmed only by the ignored HID smoke test.
- **Never commit real user data, logs or device serial numbers.**

## Commits

- Format `type(scope): summary`, where type is `feat`, `fix`, `refactor`, `test`, `docs` or `chore` and scope is the area, for example `feat(input): scroll filter separating mouse wheel from trackpad events`.
- The body says why, and which spec section or plan task it implements.
- End with the `Co-Authored-By:` trailer the session supplies.
- One concern per commit. If the message says "also", split it.
- One branch per phase or feature (`phase-2-input-engine`). Merge to `main` only with the maintainer's approval.
- **Never commit:** `dist/`, `src-tauri/target/`, `src-tauri/gen/`, `.superpowers/`, `.env*`, signing identities, certificates, or upstream source.
- UI changes: describe the before and after in the body. Attach screenshots when you can capture them.

## Taste

- Complexity belongs in pure, tested core code. Adapters stay boring.
- Prefer a `Result` with a real error type over `bool` or `String` for new code. Existing `String` errors are known debt.
- `expect` is for startup invariants only, with a message saying what broke. Never use `unwrap`/`expect` on the input hot path.
- Comments explain **why** (a macOS quirk, a spec reference), never what the next line does.
- Tests name the behaviour: `unplugging_last_mouse_disables_taps`, not `test_engine_3`.
- **User-facing messages** say what happened and what to do, in plain words:
  - Bad: `Error: tccutil exited with 1`
  - Good: `Couldn't reset the permission: tccutil exited with 1`
  - Bad: `Accessibility API not trusted`
  - Good: `Turn on MouseTune in System Settings → Privacy & Security → Accessibility.`
