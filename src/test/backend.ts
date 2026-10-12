import { mockIPC } from "@tauri-apps/api/mocks";
import type { Config, ConfigView, Device, General as GeneralSettings, InputStatus, Remap, Status } from "../bindings";

export function device(overrides: Partial<Device> = {}): Device {
  return { id: "1a", name: "MX Master 3", manufacturer: "Logitech", button_count: 5, has_wheel: true, ...overrides };
}

export function inputStatus(overrides: Partial<InputStatus> = {}): InputStatus {
  return {
    taps: { buttons: false, scroll: false, flags: false },
    devices: [],
    scroll_stats: { mouse_wheel: 0, passed_through: 0 },
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
  { trigger: { button: 3, level: 1, duration: "hold" }, effect: { action: { type: "SymbolicHotkey", id: "ShowDesktop" } } },
  { trigger: { button: 4, level: 1, duration: "click" }, effect: { action: { type: "Navigate", direction: "Back" } } },
  { trigger: { button: 5, level: 1, duration: "click" }, effect: { action: { type: "Navigate", direction: "Forward" } } },
];

export function config(overrides: Partial<GeneralSettings> = {}): Config {
  return {
    version: 1,
    general: { buttons_enabled: true, scroll_enabled: true, ...overrides },
    buttons: { remaps: defaultRemaps },
  };
}

export function configView(overrides: Partial<ConfigView> = {}): ConfigView {
  return { config: config(), recovery: null, ...overrides };
}

type Handler = (args: unknown) => unknown;

/** Mocks the Rust backend. Every command must be handled; calls are recorded. */
export function mockBackend(handlers: Record<string, Handler>) {
  const calls: string[] = [];
  const all: Record<string, Handler> = { get_config: () => configView(), ...handlers };
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
