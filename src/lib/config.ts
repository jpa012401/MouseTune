import type { BindingProblem, ConfigError, Remap, RemapProblem } from "../bindings";
import { remapLabel } from "./remaps";
import { modifierKeyLabel } from "./scroll";

function isConfigError(error: unknown): error is ConfigError {
  if (typeof error !== "object" || error === null || !("kind" in error)) return false;
  return (
    error.kind === "Io" ||
    error.kind === "UnsupportedVersion" ||
    error.kind === "InvalidRemap" ||
    error.kind === "DuplicateScrollModifier" ||
    error.kind === "TimingOutOfRange" ||
    error.kind === "InvalidBinding"
  );
}

function remapProblemMessage(reason: RemapProblem, row: Remap | undefined): string {
  switch (reason) {
    case "DuplicateTrigger":
      return `${row ? remapLabel(row).trigger : "a button and click"} is set twice.`;
    case "ButtonNotRemappable":
      return "left and right buttons can’t be remapped.";
    case "ButtonOutOfRange":
      return "MouseTune supports buttons up to 32.";
    case "LevelOutOfRange":
      return "only single, double and triple clicks are supported.";
    case "EmptyKeyCombo":
      return "that shortcut has no key.";
    case "ClicksOutOfRange":
      return "a mouse click can only be single or double.";
  }
}

/** The error carries the list index, not the button, so the copy says "that button". */
function bindingProblemMessage(reason: BindingProblem): string {
  switch (reason) {
    case "Duplicate":
      return "that button already has that kind of binding.";
    case "ButtonNotRemappable":
      return "left and right buttons can’t be remapped.";
    case "ButtonOutOfRange":
      return "MouseTune supports buttons up to 32.";
    case "HoldThroughConflict":
      return "a button that acts as another button can’t have other bindings.";
    case "EmptyDiscrete":
      return "choose at least one direction for separate actions.";
    case "InvalidAction":
      return "one of the drag’s actions can’t be used.";
  }
}

/** `error` is usually a `ConfigError`, but a broken bridge can reject with anything. */
export function configErrorMessage(error: unknown, remaps: Remap[] = []): string {
  if (!isConfigError(error)) return `Couldn’t save your settings: ${String(error)}`;
  const reason =
    error.kind === "Io"
      ? error.message
      : error.kind === "UnsupportedVersion"
        ? `settings version ${error.found} isn’t supported (expected ${error.expected})`
        : error.kind === "DuplicateScrollModifier"
          ? `${modifierKeyLabel(error.key)} is already used for another scroll modifier.`
          : error.kind === "TimingOutOfRange"
            ? `${error.field === "Hold" ? "Hold delay" : "Double-click speed"} must be between 150 and 600 ms.`
            : error.kind === "InvalidBinding"
              ? bindingProblemMessage(error.reason)
              : remapProblemMessage(error.reason, remaps[error.index]);
  return `Couldn’t save your settings: ${reason}`;
}
