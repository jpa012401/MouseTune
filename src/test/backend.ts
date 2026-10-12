import { mockIPC } from "@tauri-apps/api/mocks";
import type { Config, ConfigView, CurvePreview, Device, General as GeneralSettings, InputStatus, Remap, Scroll as ScrollSettings, Status, Timing } from "../bindings";

export function device(overrides: Partial<Device> = {}): Device {
  return { id: "1a", name: "MX Master 3", manufacturer: "Logitech", button_count: 5, has_wheel: true, ...overrides };
}

export function inputStatus(overrides: Partial<InputStatus> = {}): InputStatus {
  return {
    taps: { buttons: false, scroll: false, flags: false, keys: false },
    devices: [],
    scroll_stats: { mouse_wheel: 0, passed_through: 0 },
    actions: { performed: 0, dropped: 0, failed: 0 },
    degraded: false,
    ...overrides,
  };
}

export function status(overrides: Partial<Status> = {}): Status {
  return {
    app_version: "0.1.0",
    os_version: { major: 27, minor: 0, patch: 1 },
    accessibility_trusted: true,
    input: inputStatus(),
    ...overrides,
  };
}

export const defaultRemaps: Remap[] = [
  { trigger: { button: 3, level: 1, duration: "click" }, effect: { action: { type: "SymbolicHotkey", id: "MissionControl" } } },
  { trigger: { button: 4, level: 1, duration: "click" }, effect: { action: { type: "Navigate", direction: "Back" } } },
  { trigger: { button: 5, level: 1, duration: "click" }, effect: { action: { type: "Navigate", direction: "Forward" } } },
];

export function config(overrides: Partial<GeneralSettings> = {}): Config {
  return {
    version: 1,
    general: { buttons_enabled: true, scroll_enabled: true, freeze_pointer_during_drag: true, ...overrides },
    buttons: {
      remaps: defaultRemaps,
      drags: [{ button: 3, effect: { type: "SpacesAndMissionControl" } }],
      scroll_bindings: [],
      hold_through: [],
    },
    scroll: {
      smooth: true,
      smoothness: "regular",
      speed: "medium",
      trackpad_simulation: true,
      invert_vertical: false,
      invert_horizontal: false,
      precise: false,
      keyboard_modifiers: { horizontal: "Shift", precise: "Option", swift: null, zoom: null },
    },
    timing: { hold_ms: 250, double_click_ms: 260 },
    nav_overrides: {
      "com.apple.AppStore": "CmdBracket",
      "com.apple.Music": "CmdBracket",
      "com.apple.Preview": "CmdBracket",
      "com.apple.Safari": "CmdBracket",
      "com.apple.finder": "CmdBracket",
      "com.apple.systempreferences": "CmdBracket",
    },
  };
}

/** The default config with other timing. */
export function timingConfig(timing: Timing): Config {
  return { ...config(), timing };
}

/** The default config with some scroll settings changed. */
export function scrollConfig(patch: Partial<ScrollSettings>): Config {
  const base = config();
  return { ...base, scroll: { ...base.scroll, ...patch } };
}

export function configView(overrides: Partial<ConfigView> = {}): ConfigView {
  return { config: config(), recovery: null, ...overrides };
}

type Handler = (args: unknown) => unknown;

/** A short, hand-made preview: 3 points per line. */
export function curvePreview(): CurvePreview {
  return {
    slow: [[0, 0], [100, 60], [252, 90]],
    fast: [[0, 0], [100, 150], [230, 180]],
  };
}

/** Mocks the Rust backend. Every command must be handled; calls are recorded. */
export function mockBackend(handlers: Record<string, Handler>) {
  const calls: string[] = [];
  const all: Record<string, Handler> = { get_config: () => configView(), scroll_curve_preview: () => curvePreview(), ...handlers };
  mockIPC(
    (cmd, args) => {
      calls.push(cmd);
      const handler = all[cmd];
      if (!handler) throw new Error(`unexpected command: ${cmd}`);
      return handler(args);
    },
    { shouldMockEvents: true },
  );
  return calls;
}
