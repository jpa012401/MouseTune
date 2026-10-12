import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import App from "../App";
import type { Config, CurvePreview, Scroll as ScrollSettings } from "../bindings";
import { config, configView, curvePreview, mockBackend, scrollConfig, status } from "../test/backend";

function renderScroll() {
  window.location.hash = "#/scroll";
  return render(<App />);
}

/** Records each saved config and answers with it, as Rust does. */
function saving() {
  const saved: Config[] = [];
  const set_config = (args: unknown) => {
    const next = (args as { config: Config }).config;
    saved.push(next);
    return next;
  };
  return { saved, set_config };
}

function group(name: string) {
  return within(screen.getByRole("radiogroup", { name }));
}

describe("Scroll page", () => {
  it("shows the saved settings", async () => {
    mockBackend({
      get_status: () => status(),
      get_config: () => configView({ config: scrollConfig({ smoothness: "high", invert_horizontal: true }) }),
    });
    renderScroll();
    expect((await screen.findByRole("switch", { name: "Smooth scrolling" })).getAttribute("aria-checked")).toBe("true");
    expect(group("Smoothness").getByRole("radio", { name: "High" }).getAttribute("aria-checked")).toBe("true");
    expect(group("Speed").getByRole("radio", { name: "Medium" }).getAttribute("aria-checked")).toBe("true");
    expect(screen.getByRole("switch", { name: "Invert horizontal" }).getAttribute("aria-checked")).toBe("true");
    expect((screen.getByRole("combobox", { name: "Scroll horizontally" }) as HTMLSelectElement).value).toBe("Shift");
    expect((screen.getByRole("combobox", { name: "Scroll faster" }) as HTMLSelectElement).value).toBe("");
  });

  it("picking a speed saves it", async () => {
    const { saved, set_config } = saving();
    mockBackend({ get_status: () => status(), set_config });
    renderScroll();
    await screen.findByRole("radiogroup", { name: "Speed" });
    fireEvent.click(group("Speed").getByRole("radio", { name: "Low" }));
    await waitFor(() => expect(saved).toEqual([scrollConfig({ speed: "low" })]));
  });

  it("the precise switch saves the precise setting", async () => {
    const { saved, set_config } = saving();
    mockBackend({ get_status: () => status(), set_config });
    renderScroll();
    fireEvent.click(await screen.findByRole("switch", { name: "Precise when scrolling slowly" }));
    await waitFor(() => expect(saved).toEqual([scrollConfig({ precise: true })]));
  });

  it("choosing a key for Scroll faster saves the binding", async () => {
    const { saved, set_config } = saving();
    mockBackend({ get_status: () => status(), set_config });
    renderScroll();
    fireEvent.change(await screen.findByRole("combobox", { name: "Scroll faster" }), { target: { value: "Control" } });
    await waitFor(() =>
      expect(saved).toEqual([
        scrollConfig({ keyboard_modifiers: { horizontal: "Shift", precise: "Option", swift: "Control", zoom: null } }),
      ]),
    );
  });

  it("a key bound to another effect can't be picked again", async () => {
    mockBackend({ get_status: () => status() });
    renderScroll();
    const faster = within(await screen.findByRole("combobox", { name: "Scroll faster" }));
    expect((faster.getByRole("option", { name: "⇧ Shift" }) as HTMLOptionElement).disabled).toBe(true);
    expect((faster.getByRole("option", { name: "⌥ Option" }) as HTMLOptionElement).disabled).toBe(true);
    expect((faster.getByRole("option", { name: "⌃ Control" }) as HTMLOptionElement).disabled).toBe(false);
  });

  it("with smoothing off, its options and the modifiers are disabled and nothing is plotted", async () => {
    const calls = mockBackend({
      get_status: () => status(),
      get_config: () => configView({ config: scrollConfig({ smooth: false }) }),
    });
    renderScroll();
    await screen.findByRole("switch", { name: "Smooth scrolling" });
    expect((group("Smoothness").getByRole("radio", { name: "Regular" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("combobox", { name: "Scroll precisely" }) as HTMLSelectElement).disabled).toBe(true);
    expect(screen.getByText("Modifiers need smooth scrolling.")).toBeTruthy();
    expect((screen.getByRole("switch", { name: "Invert vertical" }) as HTMLButtonElement).disabled).toBe(false);
    expect(screen.queryByRole("img")).toBeNull();
    expect(calls).not.toContain("scroll_curve_preview");
  });

  it("with scrolling off, says so and disables everything", async () => {
    const off = { ...config({ scroll_enabled: false }) };
    mockBackend({ get_status: () => status(), get_config: () => configView({ config: off }) });
    renderScroll();
    expect(await screen.findByText("Scrolling is off. Turn it on in General or the menu bar.")).toBeTruthy();
    expect((screen.getByRole("switch", { name: "Smooth scrolling" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("switch", { name: "Invert vertical" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("with scrolling off but smoothing on, the hint is about unassigned keys", async () => {
    // config({ scroll_enabled: false }) keeps smooth: true.
    mockBackend({ get_status: () => status(), get_config: () => configView({ config: config({ scroll_enabled: false }) }) });
    renderScroll();
    expect(
      await screen.findByText("Keys you don’t assign pass through, so ⌃ + scroll still zooms the screen if Accessibility Zoom is on."),
    ).toBeTruthy();
    expect(screen.queryByText("Modifiers need smooth scrolling.")).toBeNull();
    expect((screen.getByRole("combobox", { name: "Scroll precisely" }) as HTMLSelectElement).disabled).toBe(true);
  });

  it("plots the curve with a readable summary", async () => {
    mockBackend({ get_status: () => status() });
    renderScroll();
    // Fixture: slow ends at [252, 90], fast at [230, 180].
    const plot = await screen.findByRole("img", { name: /Slow tick: 90 px over 252 ms/ });
    expect(plot.getAttribute("aria-label")).toContain("Fast tick: 180 px over 230 ms");
  });

  it("re-plots when the curve's inputs change, and only then", async () => {
    const calls = mockBackend({ get_status: () => status() });
    renderScroll();
    await screen.findByRole("img");
    const plots = () => calls.filter((c) => c === "scroll_curve_preview").length;
    expect(plots()).toBe(1);
    await act(() => emit("config-changed", scrollConfig({ smoothness: "high" })));
    await waitFor(() => expect(plots()).toBe(2));
    await act(() => emit("config-changed", scrollConfig({ smoothness: "high", invert_vertical: true })));
    expect(plots()).toBe(2);
  });

  it("a failed preview leaves the rest of the page working", async () => {
    let previews = 0;
    mockBackend({
      get_status: () => status(),
      // The first plot succeeds; re-plotting after a change fails.
      scroll_curve_preview: () => {
        previews += 1;
        if (previews > 1) throw new Error("boom");
        return curvePreview();
      },
    });
    renderScroll();
    await screen.findByRole("img");
    await act(() => emit("config-changed", scrollConfig({ smoothness: "high" })));
    await waitFor(() => expect(screen.queryByRole("img")).toBeNull());
    expect((screen.getByRole("switch", { name: "Smooth scrolling" }) as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByRole("switch", { name: "Invert vertical" }) as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByRole("combobox", { name: "Scroll horizontally" }) as HTMLSelectElement).disabled).toBe(false);
    expect((group("Smoothness").getByRole("radio", { name: "High" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("a slow, outdated preview never replaces a newer one", async () => {
    // Distinct end points so the summary tells the two answers apart.
    const old: CurvePreview = { slow: [[0, 0], [252, 90]], fast: [[0, 0], [230, 180]] };
    const fresh: CurvePreview = { slow: [[0, 0], [300, 40]], fast: [[0, 0], [280, 120]] };
    let answerOld: (preview: CurvePreview) => void = () => {};
    let previews = 0;
    mockBackend({
      get_status: () => status(),
      scroll_curve_preview: () => {
        previews += 1;
        if (previews === 1) return new Promise<CurvePreview>((resolve) => (answerOld = resolve));
        return fresh;
      },
    });
    renderScroll();
    await screen.findByRole("switch", { name: "Smooth scrolling" });
    await waitFor(() => expect(previews).toBe(1));
    await act(() => emit("config-changed", scrollConfig({ smoothness: "high" })));
    await screen.findByRole("img", { name: /Slow tick: 40 px over 300 ms/ });
    await act(async () => answerOld(old));
    expect(screen.getByRole("img").getAttribute("aria-label")).toContain("Slow tick: 40 px over 300 ms");
  });

  it.each([
    ["Smoothness Low", () => fireEvent.click(group("Smoothness").getByRole("radio", { name: "Low" })), { smoothness: "low" }],
    ["Trackpad-like momentum", () => fireEvent.click(screen.getByRole("switch", { name: "Trackpad-like momentum" })), { trackpad_simulation: false }],
    ["Invert vertical", () => fireEvent.click(screen.getByRole("switch", { name: "Invert vertical" })), { invert_vertical: true }],
    ["Invert horizontal", () => fireEvent.click(screen.getByRole("switch", { name: "Invert horizontal" })), { invert_horizontal: true }],
  ] as [string, () => void, Partial<ScrollSettings>][])("%s saves its setting", async (_, change, patch) => {
    // Defaults: smoothness regular, trackpad_simulation on, both inverts off.
    const { saved, set_config } = saving();
    mockBackend({ get_status: () => status(), set_config });
    renderScroll();
    await screen.findByRole("switch", { name: "Smooth scrolling" });
    change();
    await waitFor(() => expect(saved).toEqual([scrollConfig(patch)]));
  });

  it("a key clash from Rust shows its message", async () => {
    mockBackend({
      get_status: () => status(),
      set_config: () => {
        throw { kind: "DuplicateScrollModifier", key: "Option" };
      },
    });
    renderScroll();
    fireEvent.change(await screen.findByRole("combobox", { name: "Scroll faster" }), { target: { value: "Control" } });
    expect((await screen.findByText("Couldn’t save your settings: ⌥ Option is already used for another scroll modifier.")).getAttribute("role")).toBe("alert");
  });
});
