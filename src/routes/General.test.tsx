import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import App from "../App";
import { config, configView, device, inputStatus, mockBackend, status } from "../test/backend";

function renderGeneral() {
  window.location.hash = "#/general";
  return render(<App />);
}

describe("General page", () => {
  it("lists connected mice with their capabilities", async () => {
    mockBackend({
      get_status: () =>
        status({ input: inputStatus({ devices: [device(), device({ id: "2b", name: "Basic Mouse", manufacturer: "", button_count: 2, has_wheel: false })] }) }),
    });
    renderGeneral();
    const list = await screen.findByRole("list", { name: "Connected mice" });
    const items = within(list).getAllByRole("listitem");
    expect(items.map((li) => li.textContent)).toEqual([
      expect.stringContaining("MX Master 3"),
      expect.stringContaining("Basic Mouse"),
    ]);
    expect(items[0]!.textContent).toContain("5 buttons");
    expect(items[0]!.textContent).toContain("scroll wheel");
    expect(items[1]!.textContent).not.toContain("scroll wheel");
  });

  it("says when no mouse is connected", async () => {
    mockBackend({ get_status: () => status() });
    renderGeneral();
    expect(await screen.findByText("No mouse connected")).toBeTruthy();
  });

  it("shows which taps are running", async () => {
    mockBackend({
      get_status: () => status({ input: inputStatus({ devices: [device()], taps: { buttons: true, scroll: false, flags: true, keys: false } }) }),
    });
    renderGeneral();
    const engine = await screen.findByRole("list", { name: "Input engine" });
    expect(within(engine).getByText("Buttons").parentElement!.textContent).toContain("On");
    expect(within(engine).getByText("Scroll").parentElement!.textContent).toContain("Off");
    expect(within(engine).getByText("Modifier keys").parentElement!.textContent).toContain("On");
  });

  it("updates when a mouse is plugged in", async () => {
    mockBackend({ get_status: () => status() });
    renderGeneral();
    await screen.findByText("No mouse connected");

    await act(() =>
      emit("input-status-changed", { taps: { buttons: true, scroll: true, flags: true, keys: false }, devices: [device()] }),
    );

    // Synchronous on purpose: the page's 1 s poll returns the old backend
    // state, so only the event can have put the mouse on screen.
    expect(screen.getByText(/MX Master 3/)).toBeTruthy();
    expect(screen.queryByText("No mouse connected")).toBeNull();
  });

  it("says no mouse is connected as soon as the last one is unplugged", async () => {
    mockBackend({ get_status: () => status({ input: inputStatus({ devices: [device()] }) }) });
    renderGeneral();
    await screen.findByText(/MX Master 3/);

    await act(() =>
      emit("input-status-changed", { taps: { buttons: false, scroll: false, flags: false, keys: false }, devices: [] }),
    );

    // Synchronous on purpose: the poll would still return the mouse, so only
    // the event can have removed it.
    expect(screen.getByText("No mouse connected")).toBeTruthy();
    expect(screen.queryByText(/MX Master 3/)).toBeNull();
  });

  it("says 1 button, not 1 buttons", async () => {
    mockBackend({
      get_status: () => status({ input: inputStatus({ devices: [device({ name: "Clicker", button_count: 1, has_wheel: false })] }) }),
    });
    renderGeneral();
    const list = await screen.findByRole("list", { name: "Connected mice" });
    expect(within(list).getByText(/button/).textContent).toBe("Logitech · 1 button");
  });

  it("warns when the engine is degraded", async () => {
    mockBackend({ get_status: () => status({ input: inputStatus({ degraded: true }) }) });
    renderGeneral();
    expect((await screen.findByRole("alert")).textContent).toMatch(/recovered from an internal error/i);
  });

  it("shows scroll classification counters", async () => {
    mockBackend({
      get_status: () => status({ input: inputStatus({ scroll_stats: { mouse_wheel: 12, passed_through: 34 } }) }),
    });
    renderGeneral();
    const stats = await screen.findByRole("list", { name: "Scroll events" });
    expect(within(stats).getByText("Mouse wheel").parentElement!.textContent).toContain("12");
    expect(within(stats).getByText("Passed through").parentElement!.textContent).toContain("34");
  });
});

describe("General page — Input switches", () => {
  it("shows each switch as saved", async () => {
    mockBackend({ get_status: () => status(), get_config: () => configView({ config: config({ scroll_enabled: false }) }) });
    renderGeneral();
    expect((await screen.findByRole("switch", { name: "Mouse buttons" })).getAttribute("aria-checked")).toBe("true");
    expect(screen.getByRole("switch", { name: "Scrolling" }).getAttribute("aria-checked")).toBe("false");
  });

  it("saves a toggle and shows what Rust saved", async () => {
    const saved: unknown[] = [];
    mockBackend({
      get_status: () => status(),
      set_config: (args) => {
        saved.push((args as { config: unknown }).config);
        return config({ scroll_enabled: false });
      },
    });
    renderGeneral();
    fireEvent.click(await screen.findByRole("switch", { name: "Scrolling" }));
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Scrolling" }).getAttribute("aria-checked")).toBe("false"),
    );
    expect(saved).toEqual([config({ scroll_enabled: false })]);
  });

  it("a failed save shows the error and keeps the switch", async () => {
    mockBackend({
      get_status: () => status(),
      set_config: () => {
        throw { kind: "Io", message: "disk full" };
      },
    });
    renderGeneral();
    fireEvent.click(await screen.findByRole("switch", { name: "Scrolling" }));
    expect((await screen.findByText(/Couldn’t save your settings: disk full/)).getAttribute("role")).toBe("alert");
    expect(screen.getByRole("switch", { name: "Scrolling" }).getAttribute("aria-checked")).toBe("true");
  });

  it("a save that throws shows why instead of failing silently", async () => {
    mockBackend({
      get_status: () => status(),
      set_config: () => {
        throw new Error("boom");
      },
    });
    renderGeneral();
    fireEvent.click(await screen.findByRole("switch", { name: "Scrolling" }));
    expect((await screen.findByText(/boom/)).getAttribute("role")).toBe("alert");
    expect(screen.getByRole("switch", { name: "Scrolling" }).getAttribute("aria-checked")).toBe("true");
  });

  it("a dismiss that throws shows why", async () => {
    mockBackend({
      get_status: () => status(),
      get_config: () => configView({ recovery: { backup_file: "config.1760000000.bak.json" } }),
      dismiss_config_recovery: () => {
        throw new Error("boom");
      },
    });
    renderGeneral();
    fireEvent.click(await screen.findByRole("button", { name: "Dismiss" }));
    expect((await screen.findByText(/boom/)).getAttribute("role")).toBe("alert");
  });

  it("says when the settings couldn't be loaded", async () => {
    mockBackend({
      get_status: () => status(),
      get_config: () => {
        throw "the settings store is unavailable";
      },
    });
    renderGeneral();
    expect(
      (await screen.findByText("Couldn’t load your settings: the settings store is unavailable")).getAttribute("role"),
    ).toBe("alert");
    expect(screen.queryByRole("switch", { name: "Scrolling" })).toBeNull();
  });

  it("a change from the tray clears a stale save error", async () => {
    mockBackend({
      get_status: () => status(),
      set_config: () => {
        throw { kind: "Io", message: "disk full" };
      },
    });
    renderGeneral();
    fireEvent.click(await screen.findByRole("switch", { name: "Scrolling" }));
    await screen.findByText(/disk full/);
    await act(() => emit("config-changed", config({ buttons_enabled: false })));
    expect(screen.queryByText(/disk full/)).toBeNull();
  });

  it("a saved change removes the notice that the old file was left in place", async () => {
    let recovery: { backup_file: string | null } | null = { backup_file: null };
    mockBackend({
      get_status: () => status(),
      get_config: () => configView({ recovery }),
      set_config: () => {
        recovery = null;
        return config({ scroll_enabled: false });
      },
    });
    renderGeneral();
    await screen.findByText(/was left in place/);
    fireEvent.click(screen.getByRole("switch", { name: "Scrolling" }));
    await waitFor(() => expect(screen.queryByText(/was left in place/)).toBeNull());
    expect(screen.getByRole("switch", { name: "Scrolling" }).getAttribute("aria-checked")).toBe("false");
  });

  it("follows changes made from the tray", async () => {
    mockBackend({ get_status: () => status() });
    renderGeneral();
    await screen.findByRole("switch", { name: "Mouse buttons" });
    await act(() => emit("config-changed", config({ buttons_enabled: false })));
    expect(screen.getByRole("switch", { name: "Mouse buttons" }).getAttribute("aria-checked")).toBe("false");
  });

  it("tells the user once when the settings file was replaced", async () => {
    const calls = mockBackend({
      get_status: () => status(),
      get_config: () => configView({ recovery: { backup_file: "config.1760000000.bak.json" } }),
      dismiss_config_recovery: () => null,
    });
    renderGeneral();
    expect(await screen.findByText(/kept as config\.1760000000\.bak\.json/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByText(/kept as/)).toBeNull());
    expect(calls).toContain("dismiss_config_recovery");
  });

  it("says when the old file couldn't be backed up", async () => {
    mockBackend({ get_status: () => status(), get_config: () => configView({ recovery: { backup_file: null } }) });
    renderGeneral();
    expect(await screen.findByText(/couldn’t be backed up and was left in place/)).toBeTruthy();
  });

  it("shows the button action counters", async () => {
    mockBackend({ get_status: () => status({ input: inputStatus({ actions: { performed: 7, dropped: 1, failed: 2 } }) }) });
    renderGeneral();
    const list = await screen.findByRole("list", { name: "Button actions" });
    expect(within(list).getByText("Performed").parentElement!.textContent).toContain("7");
    expect(within(list).getByText("Dropped").parentElement!.textContent).toContain("1");
    expect(within(list).getByText("Failed").parentElement!.textContent).toContain("2");
  });
});
