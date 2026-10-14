# MouseTune

Smooth scrolling, button remapping and trackpad-style gestures for regular mice on macOS 12 Monterey through macOS 27.

Built with [Tauri v2](https://v2.tauri.app) (Rust) and React. Runs as a menu bar app.

> Inspired by [Mac Mouse Fix](https://github.com/noah-nuebling/mac-mouse-fix) by Noah Nuebling. MouseTune is an independent, clean-room implementation and contains no Mac Mouse Fix source code. See [CREDITS.md](CREDITS.md).

**Status:** Phases 0–7b are merged; the first signed release build (`pnpm release`) is still to be run. Phases 2–7 await manual hardware acceptance (see AGENTS.md "Current state"). See the [technical specification](docs/macos_mouse_utility_technical_specification.md) for the design and roadmap.

## Requirements

- macOS 12+ with Xcode Command Line Tools
- Rust (stable, 1.85+) with the `aarch64-apple-darwin` and `x86_64-apple-darwin` targets
- Node 22+ and pnpm 9

## Development

```sh
pnpm install
pnpm tauri dev
```

Rust tests also regenerate the typed IPC bindings at `src/bindings.ts`. Commit that file; CI fails if it's out of date.

```sh
pnpm typecheck && pnpm test                      # frontend
cd src-tauri && cargo clippy --all-targets && cargo test   # backend + bindings
```

Logs: `~/Library/Logs/com.johnpaulalabe.mousetune/`

### Accessibility permission while developing

MouseTune needs Accessibility access to intercept mouse input.

- **`pnpm tauri dev`** runs an unbundled binary, so macOS attributes the permission to the app that launched it. Grant Accessibility to your terminal or VS Code.
- **Bundled builds** need a stable signing identity, or each rebuild looks like a new app and the grant stops working. `pnpm release` (see [Release](#release)) refuses to build without one. To clear a stale entry:

  ```sh
  tccutil reset Accessibility com.johnpaulalabe.mousetune
  ```

## Release

`pnpm release` builds a signed, universal (Apple Silicon and Intel) `MouseTune.app` and `.dmg`.

**Before the first run**

- Xcode command-line tools, and both Rust targets: `rustup target add aarch64-apple-darwin x86_64-apple-darwin`.
- Find your signing identity: `security find-identity -v -p codesigning`.

**Run it**

```sh
APPLE_SIGNING_IDENTITY="Apple Development: <Name> (<TEAM>)" pnpm release
```

In order, it:

1. Reads the signing setup and refuses one that would give a broken or misleading build: no identity (the build would be ad-hoc signed and lose its Accessibility grant on every rebuild), an identity that isn't Apple Development or Developer ID Application, a Developer ID identity without notarization variables, an Apple Development identity with them, or a partial or doubled set of notarization variables.
2. Checks the git tree is clean, so the build traces to a commit.
3. Runs the CI checks: `pnpm typecheck`, `pnpm test`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and that `src/bindings.ts` is unchanged.
4. Runs `pnpm tauri build --target universal-apple-darwin`.
5. Checks the output: the signature, the signer, Hardened Runtime, the bundle ID, both architectures, and a minimum of macOS 12.0. A public build also checks the notarization ticket and Gatekeeper. It stops at the first failure and says what to do.
6. Prints the mode, commit, identity, both paths and the DMG's SHA-256.

The outputs are in `src-tauri/target/universal-apple-darwin/release/bundle/`:

- `macos/MouseTune.app`
- `dmg/MouseTune_<version>_universal.dmg`

**Prompts**

- `codesign` asks for keychain access the first time. Choose **Always Allow**.
- Building the DMG may ask to let Terminal (or your editor) control Finder. Allow it.

**Install**

Open the DMG, drag MouseTune to Applications, launch it, and grant Accessibility once to `/Applications/MouseTune.app`. Rebuilds signed with the same identity keep the grant.

**On another of your Macs**

A personal build isn't notarized, so the first launch needs a Gatekeeper bypass. On macOS 12–14: right-click the app → Open. On macOS 15 and later: open it once, then go to System Settings → Privacy & Security → Open Anyway.

## Release checklist

Fill in ✓ or ✗ and the date as each machine becomes available. Each column is a phase's acceptance section below (column 5 covers 5a and 5b).

| macOS | Chip | Phase 2 | 4a | 4c | 5 | 6 | 7a | 7b | Date |
|---|---|---|---|---|---|---|---|---|---|
| 12 | Apple Silicon |  |  |  |  |  |  |  |  |
| 12 | Intel |  |  |  |  |  |  |  |  |
| 13 | Apple Silicon |  |  |  |  |  |  |  |  |
| 13 | Intel |  |  |  |  |  |  |  |  |
| 14 | Apple Silicon |  |  |  |  |  |  |  |  |
| 14 | Intel |  |  |  |  |  |  |  |  |
| 15 | Apple Silicon |  |  |  |  |  |  |  |  |
| 15 | Intel |  |  |  |  |  |  |  |  |
| 26 | Apple Silicon |  |  |  |  |  |  |  |  |
| 26 | Intel |  |  |  |  |  |  |  |  |
| 27 | Apple Silicon |  |  |  |  |  |  |  |  |
| 27 | Intel |  |  |  |  |  |  |  |  |

## Going public later

1. Join the Apple Developer Program.
2. Create a **Developer ID Application** certificate, and an App Store Connect API key (Users and Access → Integrations → Team Keys, role *Developer*).
3. Set `APPLE_SIGNING_IDENTITY="Developer ID Application: …"`, `APPLE_API_KEY`, `APPLE_API_ISSUER` and `APPLE_API_KEY_PATH`.
4. `pnpm release` then notarizes, staples, and checks Gatekeeper.
5. Before publishing, settle the licence and trademark questions in AGENTS.md "Open, for the maintainer".

## Acceptance

Manual checks on real hardware, one per phase; record results in the Release checklist.

### Verifying the input engine (Phase 2)

Taps pass every event through unchanged in this phase (since Phase 4a the button tap swallows events for bound buttons, and since Phase 5a the scroll tap swallows mouse-wheel ticks while smoothing is on; see those sections below). To check the scroll filter:

1. Grant Accessibility (see above) and connect a mouse.
2. Open Settings → General. "Connected mice" lists the mouse; Input engine shows Buttons, Scroll and Modifier keys as **On**.
3. Scroll with the mouse wheel: the **Mouse wheel** counter rises.
4. Scroll with the trackpad or a Magic Mouse: only **Passed through** rises, and scrolling feels exactly as before.
5. Unplug the mouse: the list empties and the taps switch **Off**.
6. Turn off Settings → General → Scrolling (or the tray's Enable scrolling): Input engine → Scroll shows **Off**. Turn it back on.

### Verifying the button engine (Phase 4a acceptance, 5-button mouse)

1. Double-click the middle button (B3): Launchpad opens.
2. A single B3 click on a link opens it in a new tab, after a short wait for a possible double click.
3. B4 and B5 go Back and Forward in Safari (`Cmd+[`) and in Chrome.
4. B4 in Xcode and Notes does not change the text.
5. Left and right clicks are unaffected.
6. Trackpad clicks and scrolling are unaffected.
7. Turn off Settings → General → Mouse buttons (or the tray's Enable buttons): B3 does a plain middle click again.
8. Disable Mission Control's keyboard shortcut in System Settings, then click B3: Mission Control still opens (unreachable binding). This changes WindowServer hotkey state; restore your shortcut in System Settings afterwards if you changed it. Afterwards System Settings shows that shortcut as enabled with an unusual key: that is the unreachable binding MouseTune installed. Run it only with the maintainer's go-ahead.

### Verifying the Buttons editor (Phase 4b acceptance, 5-button mouse)

1. Settings → Buttons → + Add remap, press B4, choose Double click and Spotlight, then Add: a double click on B4 opens Spotlight, and a single click on B4 still goes Back after at most 260 ms.
2. Add a remap for B5 · Hold, choose Keyboard shortcut…, press ⌘⇧4, then Add: holding B5 shows the screenshot crosshair.
3. Try to add B3 · Click again: the "already set" message appears and Add stays disabled.
4. Restore defaults: the list and the button behaviour return to the Phase 4a defaults.
5. Start recording a shortcut and press ⌘Q: nothing quits, and ⌘Q is recorded.
6. Start mouse capture (+ Add remap), then close the Settings window: B3 still opens Mission Control.
7. Note whether macOS asked for Input Monitoring the first time you recorded a shortcut, and report it. It is assumed that Accessibility is enough; this step checks that.
8. Record ⌃→ for B4 · Click. Pressing B4 moves one Space right.

### Verifying smooth scrolling (Phase 5a acceptance, any wheel mouse)

1. Scroll a long page in Safari: it moves smoothly, eases out, and coasts briefly after the last tick.
2. Scroll to the top of a Safari page and keep going: it rubber-bands and springs back.
3. Spin the wheel fast, then slowly: fast spins travel much further per tick.
4. With the mouse connected, the built-in trackpad, a Magic Trackpad and a Magic Mouse (if you have one) scroll exactly as before.
5. Tray → Smooth scrolling off: plain notched scrolling again. Tray → Invert vertical scrolling on: the direction flips, with smoothing on and off.
6. If Accessibility Zoom is on, Ctrl + scroll still zooms the screen. Cmd + scroll in a browser behaves as it does without MouseTune.
7. Tray → Enable scrolling off in the middle of a coast: scrolling stops cleanly, and the next tick is plain.
8. Unplug the mouse in the middle of a coast: nothing keeps scrolling.
9. If a 60 Hz external display is connected, scroll on each screen: both are smooth.
10. With the pointer on an external display, unplug it in the middle of a coast: scrolling ends cleanly, and the next scroll is smooth.

Report how the scroll feels (too short a coast, too fast, too slow). The curve's constants are tuning knobs (spec §8.2, §8.3).

### Verifying scroll modifiers and the Scroll screen (Phase 5b acceptance, any wheel mouse)

1. Shift + scroll in Safari scrolls sideways, smoothly.
2. Option + scroll moves a few pixels per tick.
3. With Accessibility Zoom on (System Settings → Accessibility → Zoom → "Use scroll gesture with modifier keys"), Ctrl + scroll still zooms the screen.
4. Settings → Scroll → *Scroll faster* = ⌃ Control: Ctrl + scroll is now faster and Zoom no longer gets it. Set it back to None.
5. Flick the wheel down three or four times in quick succession: each flick goes further than the last.
6. Turn on *Precise when scrolling slowly*: slow ticks move a pixel or two, a fast spin still flies.
7. Every Scroll-screen control changes the feel at once; the curve plot changes with smoothness, speed and precise.
8. Toggle *Smooth scrolling* in the tray: the Scroll screen follows, and the other way round.
9. Unplug or sleep the external display in the middle of a coast: scrolling stops cleanly without another tick.

### Verifying remaps beyond hotkeys (Phase 4c acceptance, 5-button mouse)

1. Settings → Buttons: change B4 · Click to *Middle click*. B4 on a link in Safari opens it in a new tab.
2. Add B5 · Click → *Double left click*. B5 on a word selects it.
3. Add B3 · Hold → *Play/Pause*. Holding B3 plays or pauses Music.
4. Try *Volume up* and *Brightness down*: the system HUD appears and the level changes.
5. Quit MouseTune, add `"com.apple.dt.Xcode": "CmdBracket"` to `nav_overrides` in `~/Library/Application Support/com.johnpaulalabe.mousetune/config.json`, start it again: B4 in Xcode now goes back in the editor's history. Remove the entry afterwards. Type the value exactly: anything other than `"CmdBracket"` or `"MouseButtons"` makes the whole settings file fall back to the defaults, with a recovery notice.
6. Set *Hold delay* to 500 ms, including releasing the drag off the slider: a hold needs a visibly longer press. Click *Restore timing defaults*.
7. Set *Double-click speed* to 400 ms: the double-click hint says 400 ms, and slow double clicks on a button with a double-click action register.

### Verifying buttons as modifiers and drag (Phase 6 acceptance, 5-button mouse)

Dock swipe and magnify are built but switched off until you verify them (see the next section). So today Spaces and Mission Control step, and zoom uses the keyboard shortcut.

1. Hold B3 and drag left or right: the Space follows your hand, one Space per drag, like a three-finger swipe. Drag left to bring in the Space on the right; drag right for the one on the left. Drag up: Mission Control opens. Drag down: App Exposé.
2. Releasing a drag never leaves Mission Control half open.
3. During a B3 drag the pointer stays still, and it moves again on release. Turn MouseTune off from the tray mid-drag, or unplug the mouse mid-drag: the pointer is free.
4. A B3 double click still opens Launchpad. A slow start with no movement does nothing until you release.
5. Set B5 · Drag to *Two-finger swipe*. In Safari a horizontal drag goes Back or Forward, and in Maps it pans.
6. Set B4 · Scroll to *Zoom*. Holding B4 and scrolling zooms Preview in and out with ⌘= and ⌘-. A button-bound Zoom works with no ⌘ held.
7. Set B4 · Scroll to *App Switcher*: hold B4, scroll to pick an app, release B4 to switch. ⌘ is released when you let go, so no ⌘ stays down.
8. Set a button · Acts as to *Middle button*. Holding it and moving drags like a real middle button (for example, panning with autoscroll in a browser).
9. Turn off *Freeze pointer while dragging*: the pointer now moves during drags.

Also check:

- Set the Zoom key to ⌘. With ⌘ physically held, scrolling still zooms on every tick in Safari and Preview.
- Revoke Accessibility in System Settings while a stand-in button (Acts as) is held: nothing stays stuck down afterwards.

### Turning on the interactive swipe and magnify (maintainer)

Both are off. Each has one `VERIFIED_ON` list; it stays empty until you have seen the probe work on your Mac. The probes post real events, so keep the terminal trusted for Accessibility and watch the screen.

1. `cd src-tauri && cargo test dock_swipe_events_read_back -- --ignored --nocapture` (posts nothing; must pass first).
2. `cargo test dock_swipe_moves_spaces -- --ignored --nocapture`. Use the default mode, with no `MOUSETUNE_PROBE_ENCODING`: it sends what MouseTune ships. Have two Spaces open. You should see Spaces slide and Mission Control open and close, each fully.
3. `cargo test magnify_probe -- --ignored --nocapture` with Preview or Maps frontmost: the image should zoom in, then out. Then run `MOUSETUNE_PROBE_MAGNIFY=ticks cargo test magnify_probe -- --ignored --nocapture`, which sends what one wheel tick sends.

Watch for Mission Control or a Space change that stays half done, or a zoom that jumps instead of following. If the screen gets stuck, press Esc or Ctrl-↑, or run `killall Dock`.

When a probe works, tell the agent what you saw. To turn it on, follow the flip checklists next to `VERIFIED_ON` in `src-tauri/src/platform/macos/dock_swipe.rs` and `src-tauri/src/platform/macos/magnify.rs`. After that, steps 1 and 6 above become the interactive swipe and the pinch zoom.

### Verifying launch at login and settings polish (Phase 7a acceptance, macOS 27)

Steps 1–3 need an app bundle signed with a stable identity. In `pnpm tauri dev` the app isn't a bundle, so Launch at login shows as unavailable.

1. Turn on Launch at login. It appears in System Settings → General → Login Items.
2. Log out and in. MouseTune is running, with no Settings window.
3. Turn it off in System Settings. Reopen General, and the switch is off.
4. Hide the menu bar item, then Quit from Settings. Launch MouseTune from Applications: Settings opens. Turn the item back on, and the icon returns.
5. Open logs folder shows the folder in Finder. The About link opens the browser.
6. Turn launch at login off in MouseTune, then on again; it still works (checks the `notFound` mapping).

### Verifying the release build (Phase 7b acceptance, macOS 27)

1. `APPLE_SIGNING_IDENTITY="Apple Development: <Name> (<TEAM>)" pnpm release`. Answer the prompts. The run ends with the report and every check passed.
2. Open the DMG, drag MouseTune to Applications, launch it, and grant Accessibility.
3. Run "Phase 7a acceptance" steps 1–6 on this installed app.
4. Rebuild with `pnpm release` and reinstall. Accessibility is still granted.
5. Paste the release report and the results. They get recorded in AGENTS.md and the checklist.

Not covered: public mode (no Developer ID credentials yet), Intel and macOS 12–26 (the checklist), and the DMG on another Mac.

## License

[MIT](LICENSE)
