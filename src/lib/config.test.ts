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
});
