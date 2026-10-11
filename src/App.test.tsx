import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { Status } from "./bindings";
import App from "./App";

function mockStatus(status: Status) {
  mockIPC((cmd) => {
    if (cmd === "get_status") return status;
    throw new Error(`unexpected command: ${cmd}`);
  });
}

describe("App status panel", () => {
  it("shows granted accessibility", async () => {
    mockStatus({
      app_version: "0.1.0",
      os_version: { major: 27, minor: 0, patch: 1 },
      accessibility_trusted: true,
    });
    render(<App />);
    expect(await screen.findByText("Granted")).toBeTruthy();
    expect(screen.getByText("27.0.1")).toBeTruthy();
  });

  it("says System Preferences on Monterey", async () => {
    mockStatus({
      app_version: "0.1.0",
      os_version: { major: 12, minor: 7, patch: 6 },
      accessibility_trusted: false,
    });
    render(<App />);
    expect(await screen.findByText(/System Preferences/)).toBeTruthy();
  });
});
