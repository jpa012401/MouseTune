// `pnpm release`: plan → preflight → build → verify → report (Phase 7b design §1).
// Glue only: every decision is in releasePlan.ts, which has the tests.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  type CheckName,
  type Verdict,
  judgeBundleId,
  judgeGatekeeper,
  judgeHardenedRuntime,
  judgeIdentity,
  judgeMinOs,
  judgeUniversal,
  parseCodesignDetails,
  parseLipoArchs,
  parseMinOs,
  parseSpctl,
  planRelease,
} from "./releasePlan.ts";

const ROOT = join(import.meta.dirname, "..");
const TAURI = join(ROOT, "src-tauri");
const MIN_OS = "12.0"; // spec §3.4; matches bundle.macOS.minimumSystemVersion

function stop(message: string): never {
  console.error(`\n✗ ${message}`);
  process.exit(1);
}

/** Runs a step with its output shown; stops the release if it fails. */
function step(command: string, args: string[], cwd = ROOT): void {
  console.log(`\n→ ${command} ${args.join(" ")}`);
  const result = spawnSync(command, args, { cwd, stdio: "inherit", env: process.env });
  if (result.status !== 0) stop(`Release stopped: \`${command} ${args.join(" ")}\` failed.`);
}

/** Runs a tool and returns its combined output (codesign and spctl print to stderr). */
function capture(command: string, args: string[]): { status: number | null; output: string } {
  const result = spawnSync(command, args, { cwd: ROOT, encoding: "utf8" });
  return { status: result.status, output: `${result.stdout ?? ""}${result.stderr ?? ""}` };
}

function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

const plan = planRelease(process.env);
if (plan.kind === "refused") stop(plan.reason);

const conf = JSON.parse(readFileSync(join(TAURI, "tauri.conf.json"), "utf8")) as {
  productName: string;
  version: string;
  identifier: string;
};

const gitStatus = capture("git", ["status", "--porcelain"]);
if (gitStatus.status !== 0) stop("Release stopped: couldn’t read the git status (is this the MouseTune repository?).");
if (gitStatus.output.trim() !== "") {
  stop("Release stopped: the working tree has uncommitted changes. Commit or stash them so the build matches a commit.");
}

console.log(`Release: ${plan.mode} build signed by "${plan.identity}"`);
step("pnpm", ["typecheck"]);
step("pnpm", ["test"]);
step("cargo", ["fmt", "--check"], TAURI);
step("cargo", ["clippy", "--all-targets", "--", "-D", "warnings"], TAURI);
step("cargo", ["test"], TAURI);
step("git", ["diff", "--exit-code", "--", "src/bindings.ts"]);
step("pnpm", ["tauri", "build", "--target", "universal-apple-darwin"]);

const bundle = join(TAURI, "target", "universal-apple-darwin", "release", "bundle");
const app = join(bundle, "macos", `${conf.productName}.app`);
const dmg = join(bundle, "dmg", `${conf.productName}_${conf.version}_universal.dmg`);
for (const path of [app, dmg]) {
  if (!existsSync(path)) stop(`Release stopped: the build finished but ${path} is missing.`);
}
const infoPlist = join(app, "Contents", "Info.plist");
const plist = capture("/usr/libexec/PlistBuddy", ["-c", "Print :CFBundleExecutable", infoPlist]);
const executable = plist.output.trim();
if (plist.status !== 0 || executable === "") stop(`Release stopped: couldn’t read CFBundleExecutable from ${infoPlist}.`);
const binary = join(app, "Contents", "MacOS", executable);
const signature = capture("codesign", ["-dv", "--verbose=4", app]);
if (signature.status !== 0) stop(`Release stopped: \`codesign -dv\` couldn’t read the app’s signature.\n${signature.output}`);
const details = parseCodesignDetails(signature.output);

const exitOk = (status: number | null, what: string): Verdict =>
  status === 0 ? { ok: true } : { ok: false, reason: `${what} failed (exit ${status}).` };

const checks: Record<CheckName, () => Verdict> = {
  signature: () => exitOk(capture("codesign", ["--verify", "--deep", "--strict", "--verbose=2", app]).status, "codesign --verify"),
  identity: () => judgeIdentity(details, plan.identity),
  hardenedRuntime: () => judgeHardenedRuntime(details),
  bundleId: () => judgeBundleId(details, conf.identifier),
  universal: () => judgeUniversal(parseLipoArchs(capture("lipo", ["-archs", binary]).output)),
  minOs: () => judgeMinOs(parseMinOs(capture("vtool", ["-show-build", binary]).output), MIN_OS),
  stapled: () => exitOk(capture("xcrun", ["stapler", "validate", app]).status, "xcrun stapler validate"),
  gatekeeper: () => judgeGatekeeper(parseSpctl(capture("spctl", ["-a", "-vv", "-t", "exec", app]).output)),
};

console.log("\nChecks:");
for (const name of plan.checks) {
  const verdict = checks[name]();
  if (!verdict.ok) stop(`Release stopped: ${name}: ${verdict.reason}`);
  console.log(`  ✓ ${name}`);
}

const revParse = capture("git", ["rev-parse", "--short", "HEAD"]);
const commit = revParse.status === 0 && revParse.output.trim() !== "" ? revParse.output.trim() : "unknown commit";
console.log(`
Release ready (${plan.mode}, commit ${commit})
  signed by ${plan.identity}
  ${app}
  ${dmg}
  sha256 ${sha256(dmg)}  ${conf.productName}_${conf.version}_universal.dmg`);
