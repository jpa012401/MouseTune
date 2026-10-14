import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import App from "./App";
import { configView, mockBackend, status } from "./test/backend";

const untrusted = () => status({ accessibility_trusted: false });

describe("App", () => {
  it("shows settings when Accessibility is granted", async () => {
    mockBackend({ get_status: () => status() });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Buttons" })).toBeTruthy();
  });

  it("shows the settings recovery notice on the page Settings opens on", async () => {
    const calls = mockBackend({
      get_status: () => status(),
      get_config: () => configView({ recovery: { backup_file: "config.1760000000.bak.json" } }),
      dismiss_config_recovery: () => null,
    });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Buttons" })).toBeTruthy();
    expect((await screen.findByText(/kept as config\.1760000000\.bak\.json/)).closest("[role=alert]")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByText(/kept as/)).toBeNull());
    expect(calls).toContain("dismiss_config_recovery");
  });

  it("navigates between sections", async () => {
    mockBackend({ get_status: () => status() });
    render(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "About" }));
    expect(await screen.findByRole("button", { name: "Mac Mouse Fix" })).toBeTruthy();
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
