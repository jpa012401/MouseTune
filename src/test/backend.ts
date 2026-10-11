import { mockIPC } from "@tauri-apps/api/mocks";
import type { Device, InputStatus, Status } from "../bindings";

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

type Handler = (args: unknown) => unknown;

/** Mocks the Rust backend. Every command must be handled; calls are recorded. */
export function mockBackend(handlers: Record<string, Handler>) {
  const calls: string[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push(cmd);
      const handler = handlers[cmd];
      if (!handler) throw new Error(`unexpected command: ${cmd}`);
      return handler(args);
    },
    { shouldMockEvents: true },
  );
  return calls;
}
