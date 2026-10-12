# MouseTune

Smooth scrolling, button remapping and trackpad-style gestures for regular mice on macOS 12 Monterey through macOS 27.

Built with [Tauri v2](https://v2.tauri.app) (Rust) and React. Runs as a menu bar app.

> Inspired by [Mac Mouse Fix](https://github.com/noah-nuebling/mac-mouse-fix) by Noah Nuebling. MouseTune is an independent, clean-room implementation and contains no Mac Mouse Fix source code.

**Status:** Phase 0 (scaffold). See the [technical specification](macos_mouse_utility_technical_specification.md) for the design and roadmap.

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
- **Bundled builds** (`pnpm tauri build`) need a stable signing identity. Otherwise each rebuild looks like a new app and the grant stops working. Set `bundle.macOS.signingIdentity` to your Apple Development certificate. To clear a stale entry:

  ```sh
  tccutil reset Accessibility com.johnpaulalabe.mousetune
  ```

### Verifying the input engine (Phase 2)

Taps pass every event through unchanged in this phase (since Phase 4a the button tap swallows events for bound buttons, and since Phase 5a the scroll tap swallows mouse-wheel ticks while smoothing is on; see those sections below). To check the scroll filter:

1. Grant Accessibility (see above) and connect a mouse.
2. Open Settings → General. "Connected mice" lists the mouse; Input engine shows Buttons, Scroll and Modifier keys as **On**.
3. Scroll with the mouse wheel: the **Mouse wheel** counter rises.
4. Scroll with the trackpad or a Magic Mouse: only **Passed through** rises, and scrolling feels exactly as before.
5. Unplug the mouse: the list empties and the taps switch **Off**.
6. Turn off Settings → General → Scrolling (or the tray's Enable scrolling): Input engine → Scroll shows **Off**. Turn it back on.

### Verifying the button engine (Phase 4a acceptance, 5-button mouse)

1. Click the middle button (B3): Mission Control opens.
2. Hold B3 for 250 ms or more: the desktop is shown.
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
10. With the pointer on an external display, unplug it in the middle of a coast: scrolling ends cleanly, and the next scroll is smooth. (If the coast seems to stop short, note whether the next tick finishes it: a frame source that goes silent is only restarted by the next tick.)

Report how the scroll feels (too short a coast, too fast, too slow). The curve's constants are tuning knobs (spec §8.2, §8.3).

## Building

```sh
pnpm tauri build --target universal-apple-darwin
```

## License

[MIT](LICENSE)
