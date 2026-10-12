import type { ConfigError } from "../bindings";

function isConfigError(error: unknown): error is ConfigError {
  if (typeof error !== "object" || error === null || !("kind" in error)) return false;
  return error.kind === "Io" || error.kind === "UnsupportedVersion";
}

/** `error` is usually a `ConfigError`, but a broken bridge can reject with anything. */
export function configErrorMessage(error: unknown): string {
  if (!isConfigError(error)) return `Couldn’t save your settings: ${String(error)}`;
  const reason =
    error.kind === "Io"
      ? error.message
      : `settings version ${error.found} isn’t supported (expected ${error.expected})`;
  return `Couldn’t save your settings: ${reason}`;
}
