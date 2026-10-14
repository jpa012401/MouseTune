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

/**
 * The environment for `tauri build`. Tauri checks the notarization variables
 * by presence, so a blank `APPLE_ID=""` would still make it try to notarize
 * (or fail late). Blank ones are removed; codesign gets the trimmed identity.
 */
export function buildEnv(env: Env, plan: Plan): Env {
  const out: Env = { ...env };
  for (const name of [...API_KEY_VARS, ...APPLE_ID_VARS]) {
    if (!value(out, name)) delete out[name];
  }
  out.APPLE_SIGNING_IDENTITY = plan.identity;
  return out;
}

export type Verdict = { ok: true } | { ok: false; reason: string };
export type CodesignDetails = { identifier: string | null; authority: string | null; flags: string[]; adhoc: boolean };
export type SpctlResult = { accepted: boolean; source: string | null };

const PASS: Verdict = { ok: true };
const fail = (reason: string): Verdict => ({ ok: false, reason });

function field(text: string, key: string): string | null {
  const line = text.split("\n").find((l) => l.startsWith(`${key}=`));
  return line ? line.slice(key.length + 1).trim() : null;
}

/** `codesign -dv --verbose=4` output (it prints to stderr). */
export function parseCodesignDetails(text: string): CodesignDetails {
  const flags = /^CodeDirectory .*flags=0x[0-9a-f]+\(([^)]*)\)/m.exec(text)?.[1] ?? "";
  return {
    identifier: field(text, "Identifier"),
    // The first Authority line is the leaf certificate: the signer.
    authority: field(text, "Authority"),
    flags: flags && flags !== "none" ? flags.split(",") : [],
    adhoc: field(text, "Signature") === "adhoc",
  };
}

export function judgeIdentity(d: CodesignDetails, identity: string): Verdict {
  if (d.adhoc || d.authority === null) {
    return fail(`The app is ad-hoc signed, not signed by "${identity}". Check APPLE_SIGNING_IDENTITY.`);
  }
  return d.authority === identity ? PASS : fail(`The app is signed by "${d.authority}", expected "${identity}".`);
}

export function judgeHardenedRuntime(d: CodesignDetails): Verdict {
  return d.flags.includes("runtime")
    ? PASS
    : fail(
        `Hardened Runtime is off (CodeDirectory flags: ${d.flags.join(", ") || "none"}). Check bundle.macOS.hardenedRuntime in src-tauri/tauri.conf.json.`,
      );
}

export function judgeBundleId(d: CodesignDetails, expected: string): Verdict {
  return d.identifier === expected
    ? PASS
    : fail(`The app’s bundle ID is "${d.identifier ?? "none"}", expected "${expected}".`);
}

export function parseLipoArchs(text: string): string[] {
  return text.trim().split(/\s+/).filter(Boolean);
}

export function judgeUniversal(archs: string[]): Verdict {
  const universal = archs.length === 2 && archs.includes("arm64") && archs.includes("x86_64");
  return universal ? PASS : fail(`Expected a universal binary (arm64 and x86_64), found: ${archs.join(" ") || "none"}.`);
}

/**
 * `vtool -show-build`: one minimum per architecture slice. Newer binaries
 * carry LC_BUILD_VERSION (`minos`); older toolchains LC_VERSION_MIN_MACOSX
 * (`version`), which must count too, or an old slice would slip through.
 */
export function parseMinOs(text: string): string[] {
  const versions: string[] = [];
  let command = "";
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    const [key, val] = line.split(/\s+/, 2);
    if (key === "cmd") command = val ?? "";
    else if (key === "minos" && command === "LC_BUILD_VERSION") versions.push(val ?? "");
    else if (key === "version" && command === "LC_VERSION_MIN_MACOSX") versions.push(val ?? "");
  }
  return versions;
}

export function judgeMinOs(versions: string[], expected: string): Verdict {
  const ok = versions.length > 0 && versions.every((v) => v === expected);
  return ok ? PASS : fail(`Expected every slice to require macOS ${expected}, found: ${versions.join(", ") || "none"}.`);
}

/** `spctl -a -vv -t exec` output (stderr). */
export function parseSpctl(text: string): SpctlResult {
  return { accepted: /: accepted$/m.test(text), source: field(text, "source") };
}

export function judgeGatekeeper(r: SpctlResult): Verdict {
  return r.accepted && r.source === "Notarized Developer ID"
    ? PASS
    : fail(`Gatekeeper doesn’t accept the app as notarized (${r.accepted ? "accepted" : "rejected"}, source: ${r.source ?? "none"}).`);
}

/** `security find-identity -v -p codesigning` output: the identity must be listed exactly once. */
export function judgeIdentityAvailable(findIdentityOutput: string, identity: string): Verdict {
  const matches = findIdentityOutput.split("\n").filter((line) => line.includes(`"${identity}"`)).length;
  if (matches === 0) {
    return fail(
      `No valid signing identity named "${identity}" in your keychain. Run \`security find-identity -v -p codesigning\` and copy the name exactly (an expired certificate or a missing Apple intermediate certificate also hides it).`,
    );
  }
  return matches === 1
    ? PASS
    : fail(`"${identity}" matches ${matches} certificates in your keychain, so codesign can’t choose. Remove the expired or duplicate one in Keychain Access.`);
}
