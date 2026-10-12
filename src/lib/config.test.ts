import { describe, expect, it } from "vitest";
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
});
