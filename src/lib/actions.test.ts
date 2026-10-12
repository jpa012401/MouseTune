import { describe, expect, it } from "vitest";
import { ACTION_GROUPS, actionLabel, actionValue, keyComboLabel } from "./actions";

describe("action catalogue", () => {
  it("groups the pickable actions in the documented order", () => {
    expect(ACTION_GROUPS.map((g) => g.label)).toEqual(["Mission Control & Spaces", "System", "Navigation", "Mouse click", "Media keys"]);
    expect(ACTION_GROUPS[0]!.choices.map((c) => c.label).slice(0, 6)).toEqual([
      "Mission Control", "App Exposé", "Show Desktop", "Launchpad", "Move left a Space", "Move right a Space",
    ]);
    expect(ACTION_GROUPS[0]!.choices.at(-1)!.label).toBe("Desktop 16");
    expect(ACTION_GROUPS[1]!.choices.map((c) => c.label)).toEqual(["Spotlight", "Notification Center", "Look Up"]);
    expect(ACTION_GROUPS[2]!.choices.map((c) => c.label)).toEqual(["Back", "Forward"]);
  });

  it("offers every mouse click and media key", () => {
    expect(ACTION_GROUPS[3]!.choices.map((c) => c.label)).toEqual([
      "Left click", "Double left click", "Right click", "Double right click", "Middle click", "Double middle click",
      "Button 4 click", "Double Button 4 click", "Button 5 click", "Double Button 5 click",
    ]);
    expect(ACTION_GROUPS[4]!.choices.map((c) => c.label)).toEqual([
      "Play/Pause", "Next track", "Previous track", "Volume up", "Volume down", "Mute", "Brightness up", "Brightness down",
    ]);
  });

  it("gives clicks and media keys stable values and labels", () => {
    expect(actionValue({ type: "MouseClick", button: "Back", clicks: 2 })).toBe("click:Back:2");
    expect(actionValue({ type: "SystemKey", key: "Mute" })).toBe("media:Mute");
    expect(actionLabel({ type: "MouseClick", button: "Back", clicks: 2 })).toBe("Double Button 4 click");
    expect(actionLabel({ type: "MouseClick", button: "Left", clicks: 1 })).toBe("Left click");
    expect(actionLabel({ type: "SystemKey", key: "PlayPause" })).toBe("Play/Pause");
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
