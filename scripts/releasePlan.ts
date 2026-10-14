// Release decisions for `pnpm release` (Phase 7b design §1). Pure: the
// runner (`release.ts`) does the I/O. Erasable TypeScript only — Node runs it
// by stripping types.

export type Mode = "personal" | "public";
export type CheckName =
  | "signature"
  | "identity"
  | "hardenedRuntime"
  | "bundleId"
  | "universal"
  | "minOs"
  | "stapled"
  | "gatekeeper";
export type Plan = { kind: "plan"; mode: Mode; identity: string; checks: CheckName[] };
export type Refusal = { kind: "refused"; reason: string };
export type Env = Record<string, string | undefined>;

const DEVELOPMENT = "Apple Development: ";
const DEVELOPER_ID = "Developer ID Application: ";
const API_KEY_VARS = ["APPLE_API_KEY", "APPLE_API_ISSUER", "APPLE_API_KEY_PATH"];
const APPLE_ID_VARS = ["APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"];
const PERSONAL_CHECKS: CheckName[] = ["signature", "identity", "hardenedRuntime", "bundleId", "universal", "minOs"];

const refuse = (reason: string): Refusal => ({ kind: "refused", reason });

/** A leftover `VAR=""` in a shell counts as unset. */
function value(env: Env, name: string): string {
  return env[name]?.trim() ?? "";
}

export function planRelease(env: Env): Plan | Refusal {
  const identity = value(env, "APPLE_SIGNING_IDENTITY");
  if (!identity) {
    return refuse(
      "Set APPLE_SIGNING_IDENTITY to your signing identity (see `security find-identity -v -p codesigning`). Without one the build is ad-hoc signed and loses its Accessibility grant on every rebuild.",
    );
  }
  const development = identity.startsWith(DEVELOPMENT);
  if (!development && !identity.startsWith(DEVELOPER_ID)) {
    return refuse(
      'APPLE_SIGNING_IDENTITY must be an Apple Development or Developer ID Application identity, written as its full name (for example "Apple Development: Jane Doe (ABCDE12345)").',
    );
  }

  const methods = [API_KEY_VARS, APPLE_ID_VARS].map((vars) => ({
    vars,
    missing: vars.filter((name) => !value(env, name)),
  }));
  const used = methods.filter((m) => m.missing.length < m.vars.length);
  if (used.length > 1) {
    return refuse("Set only one notarization method: the APPLE_API_* variables or APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID.");
  }
  const method = used[0];
  if (method && method.missing.length > 0) {
    return refuse(`Notarization needs all of ${method.vars.join(", ")} (missing: ${method.missing.join(", ")}).`);
  }

  if (development && method) {
    return refuse(
      "Apple only notarizes Developer ID builds. Unset the APPLE_API_* / APPLE_ID variables for a personal build, or sign with a Developer ID Application identity.",
    );
  }
  if (!development && !method) {
    return refuse(
      "A Developer ID build must be notarized, or macOS blocks it on other Macs. Set the App Store Connect API key variables (README → Going public).",
    );
  }
  return development
    ? { kind: "plan", mode: "personal", identity, checks: PERSONAL_CHECKS }
    : { kind: "plan", mode: "public", identity, checks: [...PERSONAL_CHECKS, "stapled", "gatekeeper"] };
}
