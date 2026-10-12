import { describe, expect, it } from "vitest";
import { MODIFIER_KEYS, modifierKeyLabel } from "./scroll";

describe("scroll modifier keys", () => {
  it("lists the four keys in menu order with their symbols", () => {
    expect(MODIFIER_KEYS.map(modifierKeyLabel)).toEqual(["⇧ Shift", "⌥ Option", "⌃ Control", "⌘ Command"]);
  });
});
