import type { ConfigError } from "../bindings";

export function configErrorMessage(error: ConfigError): string {
  const reason =
    error.kind === "Io"
      ? error.message
      : `settings version ${error.found} isn’t supported (expected ${error.expected})`;
  return `Couldn’t save your settings: ${reason}`;
}
