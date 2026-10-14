// @vitest-environment node
import { describe, expect, it } from "vitest";
import {
  type Plan,
  buildEnv,
  judgeBundleId,
  judgeGatekeeper,
  judgeHardenedRuntime,
  judgeIdentity,
  judgeIdentityAvailable,
  judgeMinOs,
  judgeUniversal,
  parseCodesignDetails,
  parseLipoArchs,
  parseMinOs,
  parseSpctl,
  planRelease,
} from "./releasePlan.ts";

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

const DEV_SIGNED = `Executable=/Applications/MouseTune.app/Contents/MacOS/mousetune
Identifier=com.johnpaulalabe.mousetune
Format=app bundle with Mach-O universal (x86_64 arm64)
CodeDirectory v=20500 size=107613 flags=0x10000(runtime) hashes=3352+7 location=embedded
Signature size=9068
Authority=Apple Development: Jane Doe (ABCDE12345)
Authority=Apple Worldwide Developer Relations Certification Authority
Authority=Apple Root CA
TeamIdentifier=ABCDE12345`;

const NO_RUNTIME = DEV_SIGNED.replace("flags=0x10000(runtime)", "flags=0x0(none)");

const ADHOC = `Executable=/tmp/mousetune
Identifier=mousetune-45255279266372b4
Format=Mach-O thin (arm64)
CodeDirectory v=20400 size=310835 flags=0x20002(adhoc,linker-signed) hashes=9710+0 location=embedded
Signature=adhoc
TeamIdentifier=not set`;

const VTOOL_12 = `/A/MouseTune.app/Contents/MacOS/mousetune (architecture x86_64):
Load command 10
      cmd LC_BUILD_VERSION
  cmdsize 32
 platform MACOS
    minos 12.0
      sdk 27.0
   ntools 1
     tool LD
  version 711.0
/A/MouseTune.app/Contents/MacOS/mousetune (architecture arm64):
Load command 10
      cmd LC_BUILD_VERSION
  cmdsize 32
 platform MACOS
    minos 12.0
      sdk 27.0`;

const VTOOL_MIXED = `/A/x (architecture x86_64):
Load command 9
      cmd LC_VERSION_MIN_MACOSX
  cmdsize 16
  version 10.9
      sdk 10.9
/A/x (architecture arm64):
Load command 10
      cmd LC_BUILD_VERSION
  cmdsize 32
 platform MACOS
    minos 14.0
      sdk 14.0`;

describe("codesign details", () => {
  it("reads the identifier, first authority and flags", () => {
    expect(parseCodesignDetails(DEV_SIGNED)).toEqual({
      identifier: "com.johnpaulalabe.mousetune",
      authority: "Apple Development: Jane Doe (ABCDE12345)",
      flags: ["runtime"],
      adhoc: false,
    });
  });

  it("recognises an ad-hoc signature", () => {
    expect(parseCodesignDetails(ADHOC)).toEqual({
      identifier: "mousetune-45255279266372b4",
      authority: null,
      flags: ["adhoc", "linker-signed"],
      adhoc: true,
    });
  });

  it("passes the identity the app was signed with", () => {
    expect(judgeIdentity(parseCodesignDetails(DEV_SIGNED), "Apple Development: Jane Doe (ABCDE12345)")).toEqual({ ok: true });
  });

  it("names the signer when it isn't the expected identity", () => {
    expect(judgeIdentity(parseCodesignDetails(DEV_SIGNED), "Apple Development: Someone Else (ZZZZZ99999)")).toEqual({
      ok: false,
      reason: 'The app is signed by "Apple Development: Jane Doe (ABCDE12345)", expected "Apple Development: Someone Else (ZZZZZ99999)".',
    });
  });

  it("says so when the app is ad-hoc signed", () => {
    expect(judgeIdentity(parseCodesignDetails(ADHOC), "Apple Development: Jane Doe (ABCDE12345)")).toEqual({
      ok: false,
      reason: 'The app is ad-hoc signed, not signed by "Apple Development: Jane Doe (ABCDE12345)". Check APPLE_SIGNING_IDENTITY.',
    });
  });

  it("requires the hardened runtime", () => {
    expect(judgeHardenedRuntime(parseCodesignDetails(DEV_SIGNED))).toEqual({ ok: true });
    expect(judgeHardenedRuntime(parseCodesignDetails(NO_RUNTIME))).toEqual({
      ok: false,
      reason: "Hardened Runtime is off (CodeDirectory flags: none). Check bundle.macOS.hardenedRuntime in src-tauri/tauri.conf.json.",
    });
  });

  it("checks the bundle ID", () => {
    expect(judgeBundleId(parseCodesignDetails(DEV_SIGNED), "com.johnpaulalabe.mousetune")).toEqual({ ok: true });
    expect(judgeBundleId(parseCodesignDetails(ADHOC), "com.johnpaulalabe.mousetune")).toEqual({
      ok: false,
      reason: "The app’s bundle ID is \"mousetune-45255279266372b4\", expected \"com.johnpaulalabe.mousetune\".",
    });
  });
});

describe("architectures", () => {
  it("accepts arm64 and x86_64 in either order", () => {
    expect(judgeUniversal(parseLipoArchs("x86_64 arm64\n"))).toEqual({ ok: true });
    expect(judgeUniversal(parseLipoArchs("arm64 x86_64"))).toEqual({ ok: true });
  });

  it("rejects a single-architecture build", () => {
    expect(judgeUniversal(parseLipoArchs("arm64\n"))).toEqual({
      ok: false,
      reason: "Expected a universal binary (arm64 and x86_64), found: arm64.",
    });
  });
});

describe("minimum macOS", () => {
  it("reads one minimum per slice", () => {
    expect(parseMinOs(VTOOL_12)).toEqual(["12.0", "12.0"]);
  });

  it("reads LC_VERSION_MIN_MACOSX slices too", () => {
    expect(parseMinOs(VTOOL_MIXED)).toEqual(["10.9", "14.0"]);
  });

  it("requires every slice to target macOS 12.0", () => {
    expect(judgeMinOs(["12.0", "12.0"], "12.0")).toEqual({ ok: true });
    expect(judgeMinOs(parseMinOs(VTOOL_MIXED), "12.0")).toEqual({
      ok: false,
      reason: "Expected every slice to require macOS 12.0, found: 10.9, 14.0.",
    });
    expect(judgeMinOs([], "12.0")).toEqual({ ok: false, reason: "Expected every slice to require macOS 12.0, found: none." });
  });
});

describe("Gatekeeper", () => {
  it("accepts a notarized Developer ID app", () => {
    const r = parseSpctl("/A/MouseTune.app: accepted\nsource=Notarized Developer ID\norigin=Developer ID Application: Jane Doe (ABCDE12345)\n");
    expect(r).toEqual({ accepted: true, source: "Notarized Developer ID" });
    expect(judgeGatekeeper(r)).toEqual({ ok: true });
  });

  it("rejects an accepted app that is only Developer ID signed, not notarized", () => {
    const r = parseSpctl("/A/MouseTune.app: accepted\nsource=Developer ID\n");
    expect(judgeGatekeeper(r)).toEqual({
      ok: false,
      reason: "Gatekeeper doesn’t accept the app as notarized (accepted, source: Developer ID).",
    });
  });

  it("rejects anything else", () => {
    const r = parseSpctl("/A/MouseTune.app: rejected\nsource=Unnotarized Developer ID\n");
    expect(judgeGatekeeper(r)).toEqual({
      ok: false,
      reason: "Gatekeeper doesn’t accept the app as notarized (rejected, source: Unnotarized Developer ID).",
    });
  });
});

describe("buildEnv", () => {
  const plan: Plan = { kind: "plan", mode: "personal", identity: DEV, checks: [] };

  it("removes blank notarization variables so Tauri doesn’t see them", () => {
    const env = buildEnv({ APPLE_SIGNING_IDENTITY: DEV, APPLE_ID: "", APPLE_PASSWORD: "  ", APPLE_TEAM_ID: "\t", APPLE_API_KEY: "", APPLE_API_ISSUER: " ", APPLE_API_KEY_PATH: "" }, plan);
    expect(env).toEqual({ APPLE_SIGNING_IDENTITY: DEV });
  });

  it("keeps non-blank notarization variables for a public release", () => {
    const publicPlan: Plan = { kind: "plan", mode: "public", identity: DEVID, checks: [] };
    expect(buildEnv({ APPLE_SIGNING_IDENTITY: DEVID, ...API }, publicPlan)).toEqual({ APPLE_SIGNING_IDENTITY: DEVID, ...API });
  });

  it("passes codesign the trimmed identity", () => {
    expect(buildEnv({ APPLE_SIGNING_IDENTITY: `  ${DEV} ` }, plan).APPLE_SIGNING_IDENTITY).toBe(DEV);
  });

  it("keeps unrelated variables and does not change its input", () => {
    const input = { PATH: "/usr/bin", APPLE_SIGNING_IDENTITY: ` ${DEV}`, APPLE_ID: "" };
    const out = buildEnv(input, plan);
    expect(out.PATH).toBe("/usr/bin");
    expect(input).toEqual({ PATH: "/usr/bin", APPLE_SIGNING_IDENTITY: ` ${DEV}`, APPLE_ID: "" });
  });
});

describe("judgeIdentityAvailable", () => {
  const LISTING = `  1) 1A2B3C4D5E6F708192A3B4C5D6E7F8091A2B3C4D "${DEV}"
     1 valid identities found
`;

  it("passes when the identity appears once", () => {
    expect(judgeIdentityAvailable(LISTING, DEV)).toEqual({ ok: true });
  });

  it("does not accept a partial name", () => {
    expect(judgeIdentityAvailable(LISTING, "Apple Development: Jane")).toEqual({
      ok: false,
      reason:
        'No valid signing identity named "Apple Development: Jane" in your keychain. Run `security find-identity -v -p codesigning` and copy the name exactly (an expired certificate or a missing Apple intermediate certificate also hides it).',
    });
  });

  it("fails when two certificates share the name", () => {
    const twice = `  1) 1A2B3C4D5E6F708192A3B4C5D6E7F8091A2B3C4D "${DEV}"
  2) 9F8E7D6C5B4A39281706F5E4D3C2B1A099887766 "${DEV}"
     2 valid identities found
`;
    expect(judgeIdentityAvailable(twice, DEV)).toEqual({
      ok: false,
      reason: `"${DEV}" matches 2 certificates in your keychain, so codesign can’t choose. Remove the expired or duplicate one in Keychain Access.`,
    });
  });

  it("fails when the keychain has no valid identities", () => {
    expect(judgeIdentityAvailable("     0 valid identities found\n", DEV)).toMatchObject({
      ok: false,
      reason: expect.stringContaining(`No valid signing identity named "${DEV}"`),
    });
  });
});
