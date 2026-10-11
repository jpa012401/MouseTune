import { describe, expect, it } from "vitest";
import { act, render, screen, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import App from "../App";
import { device, inputStatus, mockBackend, status } from "../test/backend";

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
      get_status: () => status({ input: inputStatus({ devices: [device()], taps: { buttons: true, scroll: false, flags: true } }) }),
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
      emit("input-status-changed", { taps: { buttons: true, scroll: true, flags: true }, devices: [device()] }),
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
      emit("input-status-changed", { taps: { buttons: false, scroll: false, flags: false }, devices: [] }),
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
