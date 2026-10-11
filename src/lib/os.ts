import type { OsVersion } from "../bindings";

/** Ventura renamed System Preferences to System Settings (spec §3.4). */
export function settingsAppName(os: OsVersion): string {
  return os.major < 13 ? "System Preferences" : "System Settings";
}
