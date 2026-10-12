import type { KeyModifier } from "../bindings";

/** The keys a scroll effect can be bound to, in menu order. */
export const MODIFIER_KEYS: KeyModifier[] = ["Shift", "Option", "Control", "Command"];

const LABELS: Record<KeyModifier, string> = {
  Shift: "⇧ Shift",
  Option: "⌥ Option",
  Control: "⌃ Control",
  Command: "⌘ Command",
};

export function modifierKeyLabel(key: KeyModifier): string {
  return LABELS[key];
}
