# MouseTune — working agreement

macOS mouse utility (Tauri v2 + Rust + React). Design and roadmap: `macos_mouse_utility_technical_specification.md`. Read the relevant spec section before starting a task.

## Non-negotiables

### TDD (red → green → refactor)
- No production code without a failing test first. Run it and watch it fail for the expected reason (in Rust: declare the API with `todo!()` so the test fails at runtime, not at compile time).
- Write the minimal code to pass, then refactor with the tests green.
- Test behaviour, not wording or constants. Assert requirements ("onboarding reacts within 500 ms"), not decisions (`INTERVAL == 500`).
- Derive expected values by hand; never compute them with the code under test.
- Retrofitting tests onto existing behaviour: prove each test can fail by mutating the production code, watch it fail, then restore.
- A bug fix starts with a failing test that reproduces it.

### SOLID / DRY
- **Ports and adapters at OS boundaries.** Every macOS API (FFI, private CGS/SkyLight, IOKit, `Command`) sits behind a small trait (e.g. `accessibility::Accessibility`). The adapter is a humble object with no logic. All decisions live in pure code tested with fakes from `src-tauri/src/test_support.rs`.
- **Tauri commands are thin.** They pull dependencies from managed state and delegate to a tested use-case function.
- **Observers for fan-out.** New reactions to an event get a new `…Observer` implementation, not edits to the emitter (see `permission_monitor::PermissionObserver`).
- **One owner per fact.** Query OS state through the injected service, never by calling `platform::*` directly from features. Shared test doubles live in `test_support.rs`, and shared TS helpers in `src/lib/`.
- Keep modules single-purpose and match the layout in spec §2.3.

### Subagent-driven development
- Plan each phase into small, independent tasks, using the `superpowers:writing-plans` skill.
- Run each task through a fresh implementer subagent (TDD), then a separate reviewer subagent, following `superpowers:subagent-driven-development`.
- The main session coordinates, reviews and verifies; it doesn't hand-write feature code.

## Commands

```sh
pnpm typecheck && pnpm test                                  # frontend
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

`cargo test` regenerates `src/bindings.ts`; commit it. CI fails on drift. Never edit it by hand.

## Conventions

- Rust edition 2024, macOS 12+ (version-gated code branches on `platform::os_version`, spec §3.4).
- Frontend targets Safari 15 (Monterey WebKit): Tailwind v3.4, no `:has()` or container queries.
- Unsafe FFI: every `unsafe` block carries a `// SAFETY:` comment. Callbacks invoked by the OS wrap their bodies in `catch_unwind`.
- Commits are authored as `John Paul Alabe <alabejohnpaul@gmail.com>`. Don't copy Mac Mouse Fix source (spec §13).
