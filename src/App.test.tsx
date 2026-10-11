import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { Status } from "./bindings";
import App from "./App";

function status(overrides: Partial<Status> = {}): Status {
  return {
    app_version: "0.1.0",
    os_version: { major: 27, minor: 0, patch: 1 },
    accessibility_trusted: true,
    input: {
      taps: { buttons: false, scroll: false, flags: false },
      devices: [],
      scroll_stats: { mouse_wheel: 0, passed_through: 0 },
      degraded: false,
    },
    ...overrides,
  };
}

type Handler = (args: unknown) => unknown;

/** Mocks the Rust backend. Every command must be handled; calls are recorded. */
function mockBackend(handlers: Record<string, Handler>) {
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

const untrusted = () => status({ accessibility_trusted: false });

describe("App", () => {
  it("shows settings when Accessibility is granted", async () => {
    mockBackend({ get_status: () => status() });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Buttons" })).toBeTruthy();
  });

  it("navigates between sections", async () => {
    mockBackend({ get_status: () => status() });
    render(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "About" }));
    expect(await screen.findByText(/Inspired by Mac Mouse Fix/)).toBeTruthy();
    expect(screen.getByText(/macOS 27\.0\.1/)).toBeTruthy();
  });

  it("gates on onboarding until permission arrives", async () => {
    const calls = mockBackend({ get_status: untrusted, request_accessibility: () => null });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Open System Settings" }));
    expect(calls).toContain("request_accessibility");
    expect(screen.queryByRole("link", { name: "Buttons" })).toBeNull();

    await act(() => emit("permission-changed", { accessibility_trusted: true }));
    expect(await screen.findByRole("heading", { name: "Buttons" })).toBeTruthy();
  });

  it("re-reads status when the window regains focus", async () => {
    let current = untrusted();
    mockBackend({ get_status: () => current });
    render(<App />);
    await screen.findByRole("button", { name: "Open System Settings" });

    current = status();
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });

    expect(await screen.findByRole("heading", { name: "Buttons" })).toBeTruthy();
  });

  it("says System Preferences on Monterey", async () => {
    mockBackend({
      get_status: () => status({ accessibility_trusted: false, os_version: { major: 12, minor: 7, patch: 6 } }),
    });
    render(<App />);
    expect(await screen.findByRole("button", { name: "Open System Preferences" })).toBeTruthy();
  });

  it("shows why System Settings couldn't be opened", async () => {
    mockBackend({
      get_status: untrusted,
      request_accessibility: () => {
        throw "No application knows how to open the URL";
      },
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Open System Settings" }));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "No application knows how to open the URL",
    );
  });

  it("can reset a stale permission entry", async () => {
    const calls = mockBackend({ get_status: untrusted, reset_accessibility: () => null });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Reset permission" }));
    expect(await screen.findByText(/Reset\. Turn MouseTune on again/)).toBeTruthy();
    expect(calls).toContain("reset_accessibility");
  });

  it("shows why a reset failed and doesn't claim success", async () => {
    mockBackend({
      get_status: untrusted,
      reset_accessibility: () => {
        throw "tccutil exited with 1";
      },
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Reset permission" }));
    expect((await screen.findByRole("alert")).textContent).toContain("tccutil exited with 1");
    expect(screen.queryByText(/Reset\. Turn MouseTune on again/)).toBeNull();
  });
});
