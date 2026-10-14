import { describe, expect, it } from "vitest";
import { openErrorText } from "./open";

describe("openErrorText", () => {
  it("names what macOS refused to open", () => {
    expect(openErrorText({ kind: "Refused", target: "/tmp/x" })).toBe("macOS refused to open /tmp/x");
  });

  it("explains a missing logs folder", () => {
    expect(openErrorText({ kind: "NoLogsFolder", message: "no home" })).toBe(
      "the folder couldn’t be found (no home)",
    );
  });
});
