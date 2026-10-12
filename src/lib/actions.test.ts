import { describe, expect, it } from "vitest";
import { ACTION_GROUPS, actionLabel, actionValue, keyComboLabel } from "./actions";

describe("action catalogue", () => {
  it("groups the pickable actions in the documented order", () => {
    expect(ACTION_GROUPS.map((g) => g.label)).toEqual(["Mission Control & Spaces", "System", "Navigation"]);
    expect(ACTION_GROUPS[0]!.choices.map((c) => c.label).slice(0, 6)).toEqual([
      "Mission Control", "App Exposé", "Show Desktop", "Launchpad", "Move left a Space", "Move right a Space",
    ]);
    expect(ACTION_GROUPS[0]!.choices.at(-1)!.label).toBe("Desktop 16");
    expect(ACTION_GROUPS[1]!.choices.map((c) => c.label)).toEqual(["Spotlight", "Notification Center", "Look Up"]);
    expect(ACTION_GROUPS[2]!.choices.map((c) => c.label)).toEqual(["Back", "Forward"]);
  });

  it("gives every action a stable picker value", () => {
    expect(actionValue({ type: "SymbolicHotkey", id: "LookUp" })).toBe("hotkey:LookUp");
    expect(actionValue({ type: "Navigate", direction: "Forward" })).toBe("nav:Forward");
    expect(actionValue({ type: "KeyCombo", key_code: 1, modifiers: [], key_label: "S" })).toBe("keys");
  });

  it("writes shortcuts with modifier symbols in macOS order", () => {
    expect(keyComboLabel(["Command", "Shift"], "S")).toBe("⇧⌘S");
    expect(keyComboLabel(["Command", "Control", "Option", "Shift"], "4")).toBe("⌃⌥⇧⌘4");
    expect(keyComboLabel([], "F5")).toBe("F5");
  });

  it("labels each kind of action", () => {
    expect(actionLabel({ type: "SymbolicHotkey", id: "Desktop3" })).toBe("Desktop 3");
    expect(actionLabel({ type: "Navigate", direction: "Back" })).toBe("Back");
    expect(actionLabel({ type: "KeyCombo", key_code: 21, modifiers: ["Command", "Shift"], key_label: "4" })).toBe("⇧⌘4");
  });
});
