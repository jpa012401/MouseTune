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
    ...overrides,
  };
}

function mockBackend(initial: Status) {
  const calls: string[] = [];
  mockIPC(
    (cmd) => {
      calls.push(cmd);
      switch (cmd) {
        case "get_status":
          return initial;
        case "request_accessibility":
        case "reset_accessibility":
          return null;
        default:
          throw new Error(`unexpected command: ${cmd}`);
      }
    },
    { shouldMockEvents: true },
  );
  return calls;
}

describe("App", () => {
  it("shows settings when Accessibility is granted", async () => {
    mockBackend(status());
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Buttons" })).toBeTruthy();
  });

  it("navigates between sections", async () => {
    mockBackend(status());
    render(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "About" }));
    expect(await screen.findByText(/Inspired by Mac Mouse Fix/)).toBeTruthy();
    expect(screen.getByText(/macOS 27\.0\.1/)).toBeTruthy();
  });

  it("gates on onboarding until permission arrives", async () => {
    const calls = mockBackend(status({ accessibility_trusted: false }));
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Open System Settings" }));
    expect(calls).toContain("request_accessibility");
    expect(screen.queryByRole("link", { name: "Buttons" })).toBeNull();

    await act(() => emit("permission-changed", { accessibility_trusted: true }));
    expect(await screen.findByRole("heading", { name: "Buttons" })).toBeTruthy();
  });

  it("says System Preferences on Monterey", async () => {
    mockBackend(status({ accessibility_trusted: false, os_version: { major: 12, minor: 7, patch: 6 } }));
    render(<App />);
    expect(await screen.findByRole("button", { name: "Open System Preferences" })).toBeTruthy();
  });

  it("can reset a stale permission entry", async () => {
    const calls = mockBackend(status({ accessibility_trusted: false }));
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Reset permission" }));
    expect(await screen.findByText(/Reset\. Turn MouseTune on again/)).toBeTruthy();
    expect(calls).toContain("reset_accessibility");
  });
});
