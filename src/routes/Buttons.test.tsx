import { afterEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import App from "../App";
import type { Config } from "../bindings";
import { config, configView, defaultRemaps, inputStatus, mockBackend, status, timingConfig } from "../test/backend";

/** Capture only sees presses through the buttons tap, so most tests need it on. */
function tapsOn() {
  return status({ input: inputStatus({ taps: { buttons: true, scroll: true, flags: true, keys: false, drag: false } }) });
}

function renderButtons() {
  window.location.hash = "#/buttons";
  return render(<App />);
}

describe("Buttons page", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("lists the saved remaps", async () => {
    mockBackend({ get_status: tapsOn });
    renderButtons();
    const list = await screen.findByRole("list", { name: "Button remaps" });
    const rows = within(list).getAllByRole("listitem").map((li) => li.textContent);
    expect(rows).toEqual([
      expect.stringContaining("Mission Control"),
      expect.stringContaining("Back"),
      expect.stringContaining("Forward"),
    ]);
    expect(rows[0]).toContain("Middle button · Click");
  });

  it("says so when there are no remaps", async () => {
    mockBackend({ get_status: tapsOn, get_config: () => configView({ config: { ...config(), buttons: { ...config().buttons, remaps: [] } } }) });
    renderButtons();
    expect(await screen.findByText("No buttons are remapped.")).toBeTruthy();
  });

  it("explains that buttons are off when the kill switch is off", async () => {
    mockBackend({ get_status: tapsOn, get_config: () => configView({ config: config({ buttons_enabled: false }) }) });
    renderButtons();
    expect(await screen.findByText("Mouse buttons are turned off. Turn them on in General.")).toBeTruthy();
  });
  it("can't add a remap while the buttons tap is off, but can still change rows", async () => {
    mockBackend({ get_status: () => status() });
    renderButtons();
    expect(await screen.findByText("Turn on mouse buttons in General to add a remap.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "+ Add remap" })).toBeNull();
    expect(screen.getByRole("button", { name: "Restore defaults" })).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Change" })).toHaveLength(3);
  });

  it("offers to add a remap once the buttons tap is on", async () => {
    mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null });
    renderButtons();
    expect(await screen.findByRole("button", { name: "+ Add remap" })).toBeTruthy();
    expect(screen.queryByText("Turn on mouse buttons in General to add a remap.")).toBeNull();
  });

  it("adds a remap: capture a button, pick a trigger and an action, then save", async () => {
    let saved: Config | undefined;
    mockBackend({ get_status: tapsOn, set_config: (a) => (saved = (a as { config: Config }).config), start_capture: () => null, stop_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    expect(screen.getByText("Press the mouse button you want to remap…")).toBeTruthy();
    await act(() => emit("button-captured", { button: 6 }));
    fireEvent.click(screen.getByRole("radio", { name: "Double click" }));
    fireEvent.change(screen.getByLabelText("Action"), { target: { value: "hotkey:Spotlight" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => expect(saved).toBeDefined());
    expect(saved!.buttons.remaps.at(-1)).toEqual({ trigger: { button: 6, level: 2, duration: "click" }, effect: { action: { type: "SymbolicHotkey", id: "Spotlight" } } });
  });

  it("blocks adding a trigger that already exists", async () => {
    mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    await act(() => emit("button-captured", { button: 3 }));
    fireEvent.click(screen.getByRole("radio", { name: "Click" }));
    expect(screen.getByText("Middle button · Click is already set to Mission Control. Change that row instead.")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Add" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("records a keyboard shortcut as the action", async () => {
    let saved: Config | undefined;
    mockBackend({ get_status: tapsOn, set_config: (a) => (saved = (a as { config: Config }).config), start_capture: () => null, stop_capture: () => null, start_key_capture: () => null, stop_key_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    await act(() => emit("button-captured", { button: 5 }));
    fireEvent.click(screen.getByRole("radio", { name: "Hold" }));
    fireEvent.change(screen.getByLabelText("Action"), { target: { value: "keys" } });
    expect(screen.getByText("Press a shortcut… (Esc to cancel)")).toBeTruthy();
    await act(() => emit("key-captured", { key_code: 21, modifiers: ["Command", "Shift"], key_label: "4" }));
    expect(screen.getByText("⇧⌘4")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => expect(saved!.buttons.remaps.at(-1)!.effect.action).toEqual({ type: "KeyCombo", key_code: 21, modifiers: ["Command", "Shift"], key_label: "4" }));
  });

  it("a capture timeout says so and offers to try again", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true }); // findBy* polls on real time
    mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    await act(async () => { vi.advanceTimersByTime(10_000); });
    expect(screen.getByText("No button pressed. Try again.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
    vi.useRealTimers();
  });

  it("counts down the seconds left to press a button", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    expect(screen.getByText("10 s")).toBeTruthy();
    await act(async () => { vi.advanceTimersByTime(3_000); });
    expect(screen.getByText("7 s")).toBeTruthy();
    vi.useRealTimers();
  });

  it("announces the capture prompt but not every countdown tick", async () => {
    mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    expect(screen.getByRole("status").textContent).toBe("Press the mouse button you want to remap…");
    expect(screen.getByText("10 s").getAttribute("aria-live")).toBe("off");
  });

  it("changes a row's action", async () => {
    let saved: Config | undefined;
    mockBackend({ get_status: tapsOn, set_config: (a) => (saved = (a as { config: Config }).config) });
    renderButtons();
    const row = (await screen.findAllByRole("listitem"))[0]!;
    fireEvent.click(within(row).getByRole("button", { name: "Change" }));
    fireEvent.change(within(row).getByLabelText("Action"), { target: { value: "hotkey:AppExpose" } });
    fireEvent.click(within(row).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(saved!.buttons.remaps[0]!.effect.action).toEqual({ type: "SymbolicHotkey", id: "AppExpose" }));
    expect(saved!.buttons.remaps.slice(1)).toEqual(defaultRemaps.slice(1));
  });

  it("remaps a row to a middle click", async () => {
    let saved: Config | undefined;
    mockBackend({ get_status: tapsOn, set_config: (a) => (saved = (a as { config: Config }).config) });
    renderButtons();
    const row = (await screen.findAllByRole("listitem"))[0]!;
    fireEvent.click(within(row).getByRole("button", { name: "Change" }));
    fireEvent.change(within(row).getByLabelText("Action"), { target: { value: "click:Middle:1" } });
    fireEvent.click(within(row).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(saved!.buttons.remaps[0]!.effect.action).toEqual({ type: "MouseClick", button: "Middle", clicks: 1 }),
    );
  });

  it("lists media-key and click remaps by name", async () => {
    mockBackend({
      get_status: tapsOn,
      get_config: () =>
        configView({
          config: {
            ...config(),
            buttons: {
              ...config().buttons,
              remaps: [
                { trigger: { button: 4, level: 1, duration: "click" }, effect: { action: { type: "SystemKey", key: "PlayPause" } } },
                { trigger: { button: 5, level: 1, duration: "click" }, effect: { action: { type: "MouseClick", button: "Left", clicks: 2 } } },
              ],
            },
          },
        }),
    });
    renderButtons();
    expect(await screen.findByText("Play/Pause")).toBeTruthy();
    expect(screen.getByText("Double left click")).toBeTruthy();
  });

  it("deletes a row", async () => {
    let saved: Config | undefined;
    mockBackend({ get_status: tapsOn, set_config: (a) => (saved = (a as { config: Config }).config) });
    renderButtons();
    fireEvent.click(within((await screen.findAllByRole("listitem"))[1]!).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(saved!.buttons.remaps).toEqual([defaultRemaps[0], defaultRemaps[2]]));
  });

  it("restore_defaults_keeps_the_kill_switches", async () => {
    let saved: Config | undefined;
    vi.spyOn(window, "confirm").mockReturnValue(true);
    mockBackend({ get_status: tapsOn, get_config: () => configView({ config: { ...config({ buttons_enabled: false }), buttons: { ...config().buttons, remaps: [] } } }), set_config: (a) => (saved = (a as { config: Config }).config) });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "Restore defaults" }));
    expect(window.confirm).toHaveBeenCalledWith("Replace your button remaps with the defaults?");
    await waitFor(() => expect(saved!.buttons.remaps).toEqual(defaultRemaps));
    expect(saved!.general.buttons_enabled).toBe(false);
  });

  it("cancelling restore defaults changes nothing", async () => {
    const calls = mockBackend({ get_status: tapsOn });
    vi.spyOn(window, "confirm").mockReturnValue(false);
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "Restore defaults" }));
    expect(calls).not.toContain("set_config");
  });

  it("a failed save keeps the list and shows why", async () => {
    mockBackend({ get_status: tapsOn, set_config: () => { throw { kind: "Io", message: "disk full" }; } });
    renderButtons();
    fireEvent.click(within((await screen.findAllByRole("listitem"))[0]!).getByRole("button", { name: "Delete" }));
    expect(await screen.findByText("Couldn’t save your settings: disk full")).toBeTruthy();
    expect(screen.getAllByRole("listitem")).toHaveLength(3);
  });

  it("leaving the page stops a capture", async () => {
    const calls = mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null });
    const { unmount } = renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    unmount();
    await waitFor(() => expect(calls).toContain("stop_capture"));
  });

  it("warns when single clicks will wait for a double click", async () => {
    mockBackend({ get_status: tapsOn, get_config: () => configView({ config: { ...config(), buttons: { ...config().buttons, remaps: [
      defaultRemaps[1]!, { trigger: { button: 4, level: 2, duration: "click" }, effect: { action: { type: "SymbolicHotkey", id: "Spotlight" } } },
    ] } } }) });
    renderButtons();
    expect(await screen.findByText("Single clicks on this button will wait up to 260 ms.")).toBeTruthy();
  });

  it("shows why the settings couldn't load", async () => {
    mockBackend({ get_status: tapsOn, get_config: () => { throw "no file access"; } });
    renderButtons();
    expect(await screen.findByText("Couldn’t load your settings: no file access")).toBeTruthy();
  });

  it("shows why listening for a button couldn't start", async () => {
    mockBackend({ get_status: tapsOn, start_capture: () => { throw "tap unavailable"; } });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    expect(await screen.findByText("Couldn’t start listening for a button: tap unavailable")).toBeTruthy();
  });

  it("changing a row closes the add panel and stops its capture", async () => {
    const calls = mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    const row = screen.getAllByRole("listitem")[0]!;
    fireEvent.click(within(row).getByRole("button", { name: "Change" }));
    expect(screen.queryByText("Press the mouse button you want to remap…")).toBeNull();
    expect(within(row).getByLabelText("Action")).toBeTruthy();
    await waitFor(() => expect(calls).toContain("stop_capture"));
  });

  it("opening the add panel closes a row being changed and stops its recording", async () => {
    const calls = mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null, start_key_capture: () => null, stop_key_capture: () => null });
    renderButtons();
    const row = (await screen.findAllByRole("listitem"))[0]!;
    fireEvent.click(within(row).getByRole("button", { name: "Change" }));
    fireEvent.change(within(row).getByLabelText("Action"), { target: { value: "keys" } });
    fireEvent.click(screen.getByRole("button", { name: "+ Add remap" }));
    expect(within(row).queryByLabelText("Action")).toBeNull();
    expect(screen.getByText("Press the mouse button you want to remap…")).toBeTruthy();
    await waitFor(() => expect(calls).toContain("stop_key_capture"));
  });

  it("the panel's Cancel stops the capture and closes the panel", async () => {
    const calls = mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByText("Press the mouse button you want to remap…")).toBeNull();
    await waitFor(() => expect(calls).toContain("stop_capture"));
  });

  it("Add stays disabled until an action is chosen", async () => {
    mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    await act(() => emit("button-captured", { button: 6 }));
    fireEvent.click(screen.getByRole("radio", { name: "Click" }));
    expect((screen.getByRole("button", { name: "Add" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("a failed add keeps the panel open and shows why", async () => {
    mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null, set_config: () => { throw { kind: "Io", message: "disk full" }; } });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    await act(() => emit("button-captured", { button: 6 }));
    fireEvent.click(screen.getByRole("radio", { name: "Click" }));
    fireEvent.change(screen.getByLabelText("Action"), { target: { value: "hotkey:Spotlight" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(await screen.findByText("Couldn’t save your settings: disk full")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Add" })).toBeTruthy();
  });

  it("a cancelled shortcut recording leaves Add disabled", async () => {
    mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null, start_key_capture: () => null, stop_key_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    await act(() => emit("button-captured", { button: 6 }));
    fireEvent.click(screen.getByRole("radio", { name: "Click" }));
    fireEvent.change(screen.getByLabelText("Action"), { target: { value: "hotkey:Spotlight" } });
    fireEvent.change(screen.getByLabelText("Action"), { target: { value: "keys" } });
    await act(() => emit("key-capture-cancelled", {}));
    expect(screen.getByRole("button", { name: "Record again" })).toBeTruthy();
    expect((screen.getByRole("button", { name: "Add" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("reopening the panel after a capture starts back at step 1", async () => {
    mockBackend({ get_status: tapsOn, start_capture: () => null, stop_capture: () => null });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "+ Add remap" }));
    await act(() => emit("button-captured", { button: 6 }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByRole("button", { name: "+ Add remap" }));
    expect(screen.getByText("Press the mouse button you want to remap…")).toBeTruthy();
    expect(screen.queryByRole("radio", { name: "Click" })).toBeNull();
  });

  it("shows the timing sliders with their values", async () => {
    mockBackend({ get_status: tapsOn, get_config: () => configView({ config: timingConfig({ hold_ms: 400, double_click_ms: 300 }) }) });
    renderButtons();
    const hold = await screen.findByRole("slider", { name: "Hold delay" });
    expect((hold as HTMLInputElement).value).toBe("400");
    expect(screen.getByText("400 ms")).toBeTruthy();
    expect(screen.getByText("300 ms")).toBeTruthy();
    expect((screen.getByRole("slider", { name: "Double-click speed" }) as HTMLInputElement).min).toBe("150");
  });

  it("dragging a slider saves only when released", async () => {
    const saved: Config[] = [];
    mockBackend({ get_status: tapsOn, set_config: (a) => { const c = (a as { config: Config }).config; saved.push(c); return c; } });
    renderButtons();
    const hold = await screen.findByRole("slider", { name: "Hold delay" });
    fireEvent.input(hold, { target: { value: "300" } });
    fireEvent.input(hold, { target: { value: "400" } });
    expect(screen.getByText("400 ms")).toBeTruthy();
    expect(saved).toEqual([]);
    // The native change event: WebKit fires it once, on release.
    fireEvent.change(hold);
    await waitFor(() => expect(saved).toEqual([timingConfig({ hold_ms: 400, double_click_ms: 260 })]));
  });

  it("a committed change to a slider saves it", async () => {
    const saved: Config[] = [];
    mockBackend({ get_status: tapsOn, set_config: (a) => { const c = (a as { config: Config }).config; saved.push(c); return c; } });
    renderButtons();
    const speed = await screen.findByRole("slider", { name: "Double-click speed" });
    fireEvent.change(speed, { target: { value: "270" } });
    await waitFor(() => expect(saved).toEqual([timingConfig({ hold_ms: 250, double_click_ms: 270 })]));
  });

  it("restore timing defaults resets only the timing", async () => {
    let saved: Config | undefined;
    mockBackend({
      get_status: tapsOn,
      get_config: () => configView({ config: timingConfig({ hold_ms: 500, double_click_ms: 400 }) }),
      set_config: (a) => (saved = (a as { config: Config }).config),
    });
    renderButtons();
    fireEvent.click(await screen.findByRole("button", { name: "Restore timing defaults" }));
    await waitFor(() => expect(saved!.timing).toEqual({ hold_ms: 250, double_click_ms: 260 }));
    expect(saved!.buttons.remaps).toEqual(defaultRemaps);
  });

  it("the double-click hint uses the configured window", async () => {
    mockBackend({ get_status: tapsOn, get_config: () => configView({ config: { ...timingConfig({ hold_ms: 250, double_click_ms: 400 }), buttons: { ...config().buttons, remaps: [
      defaultRemaps[1]!, { trigger: { button: 4, level: 2, duration: "click" }, effect: { action: { type: "SymbolicHotkey", id: "Spotlight" } } },
    ] } } }) });
    renderButtons();
    expect(await screen.findByText("Single clicks on this button will wait up to 400 ms.")).toBeTruthy();
  });

  it("an out-of-range timing from Rust shows its message", async () => {
    mockBackend({ get_status: tapsOn, set_config: () => { throw { kind: "TimingOutOfRange", field: "Hold", ms: 700 }; } });
    renderButtons();
    const hold = await screen.findByRole("slider", { name: "Hold delay" });
    fireEvent.change(hold, { target: { value: "600" } });
    expect((await screen.findByText("Couldn’t save your settings: Hold delay must be between 150 and 600 ms.")).getAttribute("role")).toBe("alert");
  });

  it("a rejected save puts the slider back to the saved value", async () => {
    mockBackend({ get_status: tapsOn, set_config: () => { throw { kind: "TimingOutOfRange", field: "Hold", ms: 600 }; } });
    renderButtons();
    const hold = await screen.findByRole("slider", { name: "Hold delay" });
    fireEvent.input(hold, { target: { value: "600" } });
    fireEvent.change(hold);
    await screen.findByRole("alert");
    await waitFor(() => expect(screen.getByText("250 ms")).toBeTruthy());
    expect((hold as HTMLInputElement).value).toBe("250");
  });

  it("a hand-edited timing outside 150–600 ms shows the value in use", async () => {
    mockBackend({ get_status: tapsOn, get_config: () => configView({ config: timingConfig({ hold_ms: 50, double_click_ms: 260 }) }) });
    renderButtons();
    const hold = await screen.findByRole("slider", { name: "Hold delay" });
    expect(screen.getByText("150 ms")).toBeTruthy();
    expect((hold as HTMLInputElement).value).toBe("150");
  });
});
