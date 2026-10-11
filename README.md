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

## Building

```sh
pnpm tauri build --target universal-apple-darwin
```

## License

[MIT](LICENSE)
