import type { Action, KeyModifier, SymbolicHotkeyId } from "../bindings";

export type ActionChoice = { value: string; label: string; action: Action };

const hotkey = (id: SymbolicHotkeyId, label: string): ActionChoice => ({
  value: `hotkey:${id}`, label, action: { type: "SymbolicHotkey", id },
});

const desktops = Array.from({ length: 16 }, (_, i) =>
  hotkey(`Desktop${i + 1}` as SymbolicHotkeyId, `Desktop ${i + 1}`),
);

/** Everything the picker offers, apart from "Keyboard shortcut…". */
export const ACTION_GROUPS: { label: string; choices: ActionChoice[] }[] = [
  {
    label: "Mission Control & Spaces",
    choices: [
      hotkey("MissionControl", "Mission Control"),
      hotkey("AppExpose", "App Exposé"),
      hotkey("ShowDesktop", "Show Desktop"),
      hotkey("Launchpad", "Launchpad"),
      hotkey("SpaceLeft", "Move left a Space"),
      hotkey("SpaceRight", "Move right a Space"),
      ...desktops,
    ],
  },
  {
    label: "System",
    choices: [hotkey("Spotlight", "Spotlight"), hotkey("NotificationCenter", "Notification Center"), hotkey("LookUp", "Look Up")],
  },
  {
    label: "Navigation",
    choices: [
      { value: "nav:Back", label: "Back", action: { type: "Navigate", direction: "Back" } },
      { value: "nav:Forward", label: "Forward", action: { type: "Navigate", direction: "Forward" } },
    ],
  },
];

export function actionValue(action: Action): string {
  switch (action.type) {
    case "SymbolicHotkey":
      return `hotkey:${action.id}`;
    case "Navigate":
      return `nav:${action.direction}`;
    case "KeyCombo":
      return "keys";
  }
}

const SYMBOLS: [KeyModifier, string][] = [["Control", "⌃"], ["Option", "⌥"], ["Shift", "⇧"], ["Command", "⌘"]];

export function keyComboLabel(modifiers: KeyModifier[], keyLabel: string): string {
  return SYMBOLS.filter(([m]) => modifiers.includes(m)).map(([, s]) => s).join("") + keyLabel;
}

const LABELS = new Map(ACTION_GROUPS.flatMap((g) => g.choices).map((c) => [c.value, c.label]));

export function actionLabel(action: Action): string {
  if (action.type === "KeyCombo") return keyComboLabel(action.modifiers, action.key_label);
  return LABELS.get(actionValue(action)) ?? actionValue(action);
}
