import { describe, expect, it } from "vitest";
import type { Remap } from "../bindings";
import { defaultButtonRemaps, findTrigger, remapLabel, waitsForDoubleClick } from "./remaps";
import { defaultRemaps } from "../test/backend";

describe("remapLabel", () => {
  it("names the default remaps in plain words", () => {
    expect(defaultRemaps.map(remapLabel)).toEqual([
      { trigger: "Middle button · Click", action: "Mission Control" },
      { trigger: "Middle button · Hold", action: "Show Desktop" },
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

  it("is false with only single clicks, only multi-clicks, or on another button", () => {
    expect(waitsForDoubleClick([row(3, 1, "click"), row(3, 1, "hold")], 3)).toBe(false);
    expect(waitsForDoubleClick([row(4, 2, "click")], 4)).toBe(false);
    expect(waitsForDoubleClick([row(4, 1, "click"), row(5, 2, "click")], 4)).toBe(false);
  });
});

describe("defaultButtonRemaps", () => {
  it("matches Rust's Buttons::default()", () => {
    expect(defaultButtonRemaps).toEqual(defaultRemaps);
  });
});
