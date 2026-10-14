import type { OpenError } from "../bindings";

/** Plain-words reason an open request failed, for "Couldn’t open …: <reason>". */
export function openErrorText(err: OpenError): string {
  return err.kind === "Refused" ? `macOS refused to open ${err.target}` : `the folder couldn’t be found (${err.message})`;
}
