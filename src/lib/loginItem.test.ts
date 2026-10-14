import { describe, expect, it } from "vitest";
import { launchAtLoginHelp, launchAtLoginOn, loginItemErrorMessage } from "./loginItem";

describe("launch at login copy", () => {
  it("counts waiting-for-approval as on", () => {
    expect(launchAtLoginOn("On")).toBe(true);
    expect(launchAtLoginOn("RequiresApproval")).toBe(true);
    expect(launchAtLoginOn("Off")).toBe(false);
    expect(launchAtLoginOn("Unavailable")).toBe(false);
  });

  it("tells the user where to approve the login item", () => {
    expect(launchAtLoginHelp("RequiresApproval")).toBe("Allow MouseTune in System Settings → General → Login Items.");
  });

  it("explains a copy that can't launch at login", () => {
    expect(launchAtLoginHelp("Unavailable")).toBe("Launch at login isn’t available for this copy of MouseTune.");
  });

  it("says what the switch does otherwise", () => {
    expect(launchAtLoginHelp("Off")).toBe("Start MouseTune when you log in.");
    expect(launchAtLoginHelp("On")).toBe("Start MouseTune when you log in.");
  });

  it("names the direction and macOS's reason when a change fails", () => {
    expect(loginItemErrorMessage({ kind: "Register", message: "Operation not permitted" }, true)).toBe(
      "Couldn’t turn on launch at login: Operation not permitted",
    );
    expect(loginItemErrorMessage({ kind: "RemoveAgent", message: "busy" }, false)).toBe(
      "Couldn’t turn off launch at login: busy",
    );
  });
});
