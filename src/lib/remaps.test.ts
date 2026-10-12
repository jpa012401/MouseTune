import { describe, expect, it } from "vitest";
import { remapLabel } from "./remaps";
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
});
