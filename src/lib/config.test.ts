import { describe, expect, it } from "vitest";
import { defaultRemaps } from "../test/backend";
import { configErrorMessage } from "./config";

describe("configErrorMessage", () => {
  it("shows the I/O reason", () => {
    expect(configErrorMessage({ kind: "Io", message: "disk full" })).toBe("Couldn’t save your settings: disk full");
  });
  it("explains a version mismatch", () => {
    expect(configErrorMessage({ kind: "UnsupportedVersion", found: 7, expected: 1 })).toBe(
      "Couldn’t save your settings: settings version 7 isn’t supported (expected 1)",
    );
  });
  it("falls back to the raw reason for anything that isn't a ConfigError", () => {
    expect(configErrorMessage("the core is gone")).toBe("Couldn’t save your settings: the core is gone");
  });
  it("explains an invalid remap in plain words, naming the row", () => {
    const remaps = [defaultRemaps[2]!, defaultRemaps[2]!];
    expect(configErrorMessage({ kind: "InvalidRemap", index: 1, reason: "DuplicateTrigger" }, remaps)).toBe(
      "Couldn’t save your settings: Button 4 · Click is set twice.",
    );
    expect(configErrorMessage({ kind: "InvalidRemap", index: 0, reason: "ButtonNotRemappable" }, remaps)).toBe(
      "Couldn’t save your settings: left and right buttons can’t be remapped.",
    );
    expect(configErrorMessage({ kind: "InvalidRemap", index: 0, reason: "EmptyKeyCombo" }, remaps)).toBe(
      "Couldn’t save your settings: that shortcut has no key.",
    );
  });
  it("names the key bound twice to scroll modifiers", () => {
    expect(configErrorMessage({ kind: "DuplicateScrollModifier", key: "Option" })).toBe(
      "Couldn’t save your settings: ⌥ Option is already used for another scroll modifier.",
    );
  });
  it("explains out-of-range timing", () => {
    expect(configErrorMessage({ kind: "TimingOutOfRange", field: "Hold", ms: 700 })).toBe(
      "Couldn’t save your settings: Hold delay must be between 150 and 600 ms.",
    );
    expect(configErrorMessage({ kind: "TimingOutOfRange", field: "DoubleClick", ms: 100 })).toBe(
      "Couldn’t save your settings: Double-click speed must be between 150 and 600 ms.",
    );
  });
  it("explains a mouse click with too many clicks", () => {
    expect(configErrorMessage({ kind: "InvalidRemap", index: 0, reason: "ClicksOutOfRange" })).toBe(
      "Couldn’t save your settings: a mouse click can only be single or double.",
    );
  });
});
