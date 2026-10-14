import { describe, expect, it } from "vitest";
import type { Buttons, Remap } from "../bindings";
import { actsAsLabel, dragEffectReady, bindingConflict, bindingRows, defaultButtonRemaps, defaultButtons, dragLabel, findTrigger, remapLabel, scrollLabel, waitsForDoubleClick } from "./remaps";
import { config, defaultRemaps } from "../test/backend";

describe("remapLabel", () => {
  it("names the default remaps in plain words", () => {
    expect(defaultRemaps.map(remapLabel)).toEqual([
      { trigger: "Middle button · Double click", action: "Launchpad" },
      { trigger: "Button 4 · Click", action: "Back" },
      { trigger: "Button 5 · Click", action: "Forward" },
    ]);
  });

  it("names multi-clicks and held multi-clicks", () => {
    const action = { type: "Navigate", direction: "Back" } as const;
    expect(remapLabel({ trigger: { button: 6, level: 2, duration: "click" }, effect: { action } }).trigger).toBe("Button 6 · Double click");
    expect(remapLabel({ trigger: { button: 3, level: 3, duration: "hold" }, effect: { action } }).trigger).toBe("Middle button · Triple click and hold");
  });

  it("labels a recorded shortcut", () => {
    const action = { type: "KeyCombo", key_code: 1, modifiers: ["Command", "Shift"], key_label: "S" } as const;
    expect(remapLabel({ trigger: { button: 5, level: 1, duration: "hold" }, effect: { action: { ...action, modifiers: [...action.modifiers] } } }).action).toBe("⇧⌘S");
  });
});

const back = { type: "Navigate", direction: "Back" } as const;
const row = (button: number, level: number, duration: "click" | "hold"): Remap => ({ trigger: { button, level, duration }, effect: { action: back } });

describe("findTrigger", () => {
  const remaps = [row(3, 1, "click"), row(3, 1, "hold"), row(4, 2, "click")];

  it("finds the row with the same button, level and duration", () => {
    expect(findTrigger(remaps, { button: 3, level: 1, duration: "hold" })).toBe(1);
    expect(findTrigger(remaps, { button: 4, level: 2, duration: "click" })).toBe(2);
  });

  it("returns -1 when any part differs", () => {
    expect(findTrigger(remaps, { button: 5, level: 1, duration: "click" })).toBe(-1);
    expect(findTrigger(remaps, { button: 3, level: 2, duration: "click" })).toBe(-1);
    expect(findTrigger(remaps, { button: 4, level: 2, duration: "hold" })).toBe(-1);
    expect(findTrigger([], { button: 3, level: 1, duration: "click" })).toBe(-1);
  });
});

describe("waitsForDoubleClick", () => {
  it("is true when a button has both a single and a multi-click binding", () => {
    expect(waitsForDoubleClick([row(4, 1, "click"), row(4, 2, "click")], 4)).toBe(true);
    expect(waitsForDoubleClick([row(4, 3, "hold"), row(4, 1, "hold")], 4)).toBe(true);
  });

  it("is true with only a multi-click binding, because the router replays a plain click after the wait", () => {
    expect(waitsForDoubleClick([row(4, 2, "click")], 4)).toBe(true);
    expect(waitsForDoubleClick([row(4, 3, "hold")], 4)).toBe(true);
  });

  it("is false with only single clicks or holds, or on another button", () => {
    expect(waitsForDoubleClick([row(3, 1, "click"), row(3, 1, "hold")], 3)).toBe(false);
    expect(waitsForDoubleClick([row(4, 1, "click"), row(5, 2, "click")], 4)).toBe(false);
  });
});

describe("defaultButtonRemaps", () => {
  it("matches Rust's Buttons::default()", () => {
    expect(defaultButtonRemaps).toEqual(defaultRemaps);
  });
});

const empty: Buttons = { remaps: [], drags: [], scroll_bindings: [], hold_through: [] };

describe("binding labels", () => {
  it("names each drag, scroll and acts-as effect", () => {
    expect(dragLabel({ type: "SpacesAndMissionControl" })).toBe("Spaces & Mission Control");
    expect(dragLabel({ type: "ScrollSwipe" })).toBe("Two-finger swipe");
    expect(
      dragLabel({ type: "Discrete", left: { type: "Navigate", direction: "Back" }, right: { type: "Navigate", direction: "Forward" }, up: null, down: null }),
    ).toBe("Left: Back, Right: Forward");
    expect(scrollLabel("Zoom")).toBe("Zoom");
    expect(scrollLabel("Horizontal")).toBe("Scroll horizontally");
    expect(scrollLabel("AppSwitcher")).toBe("App Switcher");
    expect(actsAsLabel("Middle")).toBe("Middle button");
    expect(actsAsLabel("Back")).toBe("Button 4");
  });

  it("lists every binding by button", () => {
    const b: Buttons = {
      remaps: [{ trigger: { button: 3, level: 1, duration: "click" }, effect: { action: { type: "SymbolicHotkey", id: "MissionControl" } } }],
      drags: [{ button: 3, effect: { type: "SpacesAndMissionControl" } }],
      scroll_bindings: [{ button: 4, effect: "Zoom" }],
      hold_through: [{ button: 6, acts_as: "Middle" }],
    };
    expect(bindingRows(b).map((r) => `${r.trigger} → ${r.action}`)).toEqual([
      "Middle button · Click → Mission Control",
      "Middle button · Drag → Spaces & Mission Control",
      "Button 4 · Scroll → Zoom",
      "Button 6 · Acts as → Middle button",
    ]);
  });

  it("finds conflicts between bindings", () => {
    const b: Buttons = { ...empty, drags: [{ button: 3, effect: { type: "ScrollSwipe" } }], hold_through: [{ button: 6, acts_as: "Left" }] };
    expect(bindingConflict(b, 3, "drag")).toBe("Middle button · Drag is already set to Two-finger swipe. Change that row instead.");
    expect(bindingConflict(b, 3, "through")).toBe("Middle button already has other bindings, so it can’t act as another button.");
    expect(bindingConflict(b, 6, "scroll")).toBe("Button 6 acts as another button, so it can’t have other bindings.");
    expect(bindingConflict(b, 4, "scroll")).toBeNull();
  });

  it("a drag effect is ready unless separate actions has no direction", () => {
    expect(dragEffectReady(null)).toBe(false);
    expect(dragEffectReady({ type: "ScrollSwipe" })).toBe(true);
    expect(dragEffectReady({ type: "Discrete", left: null, right: null, up: null, down: null })).toBe(false);
    expect(dragEffectReady({ type: "Discrete", left: null, right: null, up: null, down: { type: "Navigate", direction: "Back" } })).toBe(true);
  });

  it("defaultButtons matches the fixture's default config", () => {
    expect(defaultButtons).toEqual(config().buttons);
  });
});
