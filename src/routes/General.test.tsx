import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import App from "../App";
import type { Config } from "../bindings";
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
      get_status: () => status({ input: inputStatus({ devices: [device()], taps: { buttons: true, scroll: false, flags: true, keys: false, drag: false } }) }),
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
      emit("input-status-changed", { taps: { buttons: true, scroll: true, flags: true, keys: false, drag: false }, devices: [device()] }),
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
      emit("input-status-changed", { taps: { buttons: false, scroll: false, flags: false, keys: false, drag: false }, devices: [] }),
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

  it("turns pointer freezing off", async () => {
    let saved: Config | undefined;
    mockBackend({ get_status: () => status(), set_config: (a) => (saved = (a as { config: Config }).config) });
    renderGeneral();
    fireEvent.click(await screen.findByRole("switch", { name: "Freeze pointer while dragging" }));
    await waitFor(() => expect(saved!.general.freeze_pointer_during_drag).toBe(false));
  });

  it("shows whether the Drag tap is running", async () => {
    mockBackend({ get_status: () => status({ input: inputStatus({ devices: [device()], taps: { buttons: true, scroll: true, flags: true, keys: false, drag: true } }) }) });
    renderGeneral();
    const engine = await screen.findByRole("list", { name: "Input engine" });
    expect(within(engine).getByText("Drag").nextSibling?.textContent).toBe("On");
  });
});

describe("General page: startup", () => {
  it("shows launch at login as macOS reports it", async () => {
    mockBackend({ get_status: () => status(), get_launch_at_login: () => "On" });
    renderGeneral();
    const toggle = await screen.findByRole("switch", { name: "Launch at login" });
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("true"));
  });

  it("asks for approval in Login Items when macOS needs it", async () => {
    mockBackend({ get_status: () => status(), get_launch_at_login: () => "RequiresApproval" });
    renderGeneral();
    expect(await screen.findByText("Allow MouseTune in System Settings → General → Login Items.")).toBeTruthy();
    expect(screen.getByRole("switch", { name: "Launch at login" }).getAttribute("aria-checked")).toBe("true");
  });

  it("disables the switch for a copy that can't launch at login", async () => {
    mockBackend({ get_status: () => status(), get_launch_at_login: () => "Unavailable" });
    renderGeneral();
    await screen.findByText("Launch at login isn’t available for this copy of MouseTune.");
    expect((screen.getByRole("switch", { name: "Launch at login" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("turns launch at login on and shows what macOS did", async () => {
    let sent: unknown;
    mockBackend({
      get_status: () => status(),
      get_launch_at_login: () => "Off",
      set_launch_at_login: (args) => {
        sent = args;
        return "RequiresApproval";
      },
    });
    renderGeneral();
    const toggle = await screen.findByRole("switch", { name: "Launch at login" });
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("false"));
    fireEvent.click(toggle);
    expect(await screen.findByText("Allow MouseTune in System Settings → General → Login Items.")).toBeTruthy();
    expect(sent).toEqual({ enabled: true });
  });

  it("shows a refused change and what macOS actually has afterwards", async () => {
    let current = "Off";
    mockBackend({
      get_status: () => status(),
      get_launch_at_login: () => current,
      set_launch_at_login: () => {
        current = "RequiresApproval"; // macOS changed state even though the call failed
        throw { kind: "Register", message: "Operation not permitted" };
      },
    });
    renderGeneral();
    const toggle = await screen.findByRole("switch", { name: "Launch at login" });
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("false"));
    fireEvent.click(toggle);
    expect(await screen.findByText("Couldn’t turn on launch at login: Operation not permitted")).toBeTruthy();
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("true"));
    expect(screen.getByText("Allow MouseTune in System Settings → General → Login Items.")).toBeTruthy();
  });

  it("can't send a second change while one is in flight", async () => {
    let calls = 0;
    let finish: (value: unknown) => void = () => {};
    mockBackend({
      get_status: () => status(),
      get_launch_at_login: () => "Off",
      set_launch_at_login: () => {
        calls += 1;
        return new Promise((resolve) => (finish = resolve));
      },
    });
    renderGeneral();
    const toggle = (await screen.findByRole("switch", { name: "Launch at login" })) as HTMLButtonElement;
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("false"));
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle.disabled).toBe(true));
    fireEvent.click(toggle);
    expect(calls).toBe(1);
    await act(async () => finish("On"));
    await waitFor(() => expect(toggle.disabled).toBe(false));
  });

  it("re-reads launch at login when the window regains focus", async () => {
    let current = "Off";
    mockBackend({ get_status: () => status(), get_launch_at_login: () => current });
    renderGeneral();
    const toggle = await screen.findByRole("switch", { name: "Launch at login" });
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("false"));
    current = "On"; // changed in System Settings while away
    await act(async () => void window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("true"));
  });

  it("hides the menu bar item and says how to get back", async () => {
    let saved: Config | undefined;
    mockBackend({
      get_status: () => status(),
      set_config: (args) => {
        saved = (args as { config: Config }).config;
        return saved;
      },
    });
    renderGeneral();
    fireEvent.click(await screen.findByRole("switch", { name: "Show menu bar item" }));
    expect(await screen.findByText("Open MouseTune from Applications or Spotlight to get back here.")).toBeTruthy();
    expect(saved?.general.show_menu_bar_item).toBe(false);
  });

  it("quits MouseTune from Troubleshooting", async () => {
    const calls = mockBackend({ get_status: () => status(), quit_app: () => null });
    renderGeneral();
    fireEvent.click(await screen.findByRole("button", { name: "Quit MouseTune" }));
    await waitFor(() => expect(calls).toContain("quit_app"));
  });

  it("opens the logs folder", async () => {
    const calls = mockBackend({ get_status: () => status(), open_logs_folder: () => null });
    renderGeneral();
    fireEvent.click(await screen.findByRole("button", { name: "Open logs folder" }));
    await waitFor(() => expect(calls).toContain("open_logs_folder"));
  });

  it("says when the logs folder couldn't be opened", async () => {
    mockBackend({
      get_status: () => status(),
      open_logs_folder: () => {
        throw { kind: "Refused", target: "/Users/me/Library/Logs/x" };
      },
    });
    renderGeneral();
    fireEvent.click(await screen.findByRole("button", { name: "Open logs folder" }));
    expect(await screen.findByText("Couldn’t open the logs folder: macOS refused to open /Users/me/Library/Logs/x")).toBeTruthy();
  });
});
