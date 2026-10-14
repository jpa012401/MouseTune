// @vitest-environment node
import { describe, expect, it } from "vitest";
import { planRelease } from "./releasePlan.ts";

const DEV = "Apple Development: Jane Doe (ABCDE12345)";
const DEVID = "Developer ID Application: Jane Doe (ABCDE12345)";
const API = { APPLE_API_KEY: "KEY123", APPLE_API_ISSUER: "issuer-uuid", APPLE_API_KEY_PATH: "/keys/AuthKey_KEY123.p8" };
const APPLE_ID = { APPLE_ID: "jane@example.com", APPLE_PASSWORD: "app-specific", APPLE_TEAM_ID: "ABCDE12345" };
const PERSONAL = ["signature", "identity", "hardenedRuntime", "bundleId", "universal", "minOs"];

describe("planRelease", () => {
  it("builds a personal release with an Apple Development identity", () => {
    expect(planRelease({ APPLE_SIGNING_IDENTITY: DEV })).toEqual({ kind: "plan", mode: "personal", identity: DEV, checks: PERSONAL });
  });

  it("builds a notarized public release with Developer ID and an API key", () => {
    expect(planRelease({ APPLE_SIGNING_IDENTITY: DEVID, ...API })).toEqual({
      kind: "plan",
      mode: "public",
      identity: DEVID,
      checks: [...PERSONAL, "stapled", "gatekeeper"],
    });
  });

  it("accepts Apple ID notarization too", () => {
    expect(planRelease({ APPLE_SIGNING_IDENTITY: DEVID, ...APPLE_ID })).toMatchObject({ kind: "plan", mode: "public" });
  });

  it("refuses to build without an identity, because ad-hoc builds lose Accessibility", () => {
    expect(planRelease({})).toEqual({
      kind: "refused",
      reason:
        "Set APPLE_SIGNING_IDENTITY to your signing identity (see `security find-identity -v -p codesigning`). Without one the build is ad-hoc signed and loses its Accessibility grant on every rebuild.",
    });
  });

  it("treats a blank identity as missing", () => {
    expect(planRelease({ APPLE_SIGNING_IDENTITY: "   " })).toMatchObject({ kind: "refused", reason: expect.stringContaining("Set APPLE_SIGNING_IDENTITY") });
  });

  it("refuses an identity given as a hash or of another kind", () => {
    const refused = {
      kind: "refused",
      reason:
        'APPLE_SIGNING_IDENTITY must be an Apple Development or Developer ID Application identity, written as its full name (for example "Apple Development: Jane Doe (ABCDE12345)").',
    };
    expect(planRelease({ APPLE_SIGNING_IDENTITY: "1A2B3C4D5E6F708192A3B4C5D6E7F8091A2B3C4D" })).toEqual(refused);
    expect(planRelease({ APPLE_SIGNING_IDENTITY: "3rd Party Mac Developer Application: Jane Doe (ABCDE12345)" })).toEqual(refused);
  });

  it("refuses to notarize a development build", () => {
    expect(planRelease({ APPLE_SIGNING_IDENTITY: DEV, ...API })).toEqual({
      kind: "refused",
      reason:
        "Apple only notarizes Developer ID builds. Unset the APPLE_API_* / APPLE_ID variables for a personal build, or sign with a Developer ID Application identity.",
    });
  });

  it("refuses a Developer ID build that wouldn't be notarized", () => {
    expect(planRelease({ APPLE_SIGNING_IDENTITY: DEVID })).toEqual({
      kind: "refused",
      reason:
        "A Developer ID build must be notarized, or macOS blocks it on other Macs. Set the App Store Connect API key variables (README → Going public).",
    });
  });

  it("names the missing notarization variables", () => {
    expect(planRelease({ APPLE_SIGNING_IDENTITY: DEVID, APPLE_API_KEY: "KEY123" })).toEqual({
      kind: "refused",
      reason: "Notarization needs all of APPLE_API_KEY, APPLE_API_ISSUER, APPLE_API_KEY_PATH (missing: APPLE_API_ISSUER, APPLE_API_KEY_PATH).",
    });
    expect(planRelease({ APPLE_SIGNING_IDENTITY: DEVID, APPLE_ID: "jane@example.com", APPLE_TEAM_ID: "ABCDE12345" })).toEqual({
      kind: "refused",
      reason: "Notarization needs all of APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID (missing: APPLE_PASSWORD).",
    });
  });

  it("refuses two notarization methods at once", () => {
    expect(planRelease({ APPLE_SIGNING_IDENTITY: DEVID, ...API, APPLE_ID: "jane@example.com" })).toEqual({
      kind: "refused",
      reason: "Set only one notarization method: the APPLE_API_* variables or APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID.",
    });
  });

  it("ignores empty leftover notarization variables in a personal build", () => {
    expect(planRelease({ APPLE_SIGNING_IDENTITY: DEV, APPLE_ID: "", APPLE_PASSWORD: "  " })).toMatchObject({ kind: "plan", mode: "personal" });
  });
});
