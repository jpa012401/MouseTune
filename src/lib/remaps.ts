import type { Action, ClickTrigger, Remap, Timing, TriggerDuration } from "../bindings";
import { actionLabel } from "./actions";

export const DEFAULT_TIMING: Timing = { hold_ms: 250, double_click_ms: 260 };

const LEVELS = ["Click", "Double click", "Triple click"];

export function buttonName(button: number): string {
  return button === 3 ? "Middle button" : `Button ${button}`;
}

/** Plain-words label for one remap row. */
export function remapLabel({ trigger, effect }: Remap): { trigger: string; action: string } {
  const level = LEVELS[trigger.level - 1] ?? `${trigger.level}× click`;
  const press = trigger.duration === "click" ? level : trigger.level === 1 ? "Hold" : `${level} and hold`;
  return { trigger: `${buttonName(trigger.button)} · ${press}`, action: actionLabel(effect.action) };
}

/** Index of the row with exactly this trigger, or -1. */
export function findTrigger(remaps: Remap[], trigger: ClickTrigger): number {
  return remaps.findIndex(
    (r) => r.trigger.button === trigger.button && r.trigger.level === trigger.level && r.trigger.duration === trigger.duration,
  );
}

/**
 * A single click must wait out the double-click window whenever the button has a multi-click binding,
 * even with no single-click row: the router replays the plain click only after the window expires.
 */
export function waitsForDoubleClick(remaps: Remap[], button: number): boolean {
  return remaps.some((r) => r.trigger.button === button && r.trigger.level >= 2);
}

const remap = (button: number, duration: TriggerDuration, action: Action): Remap => ({
  trigger: { button, level: 1, duration },
  effect: { action },
});

/**
 * Mirrors Rust's `Buttons::default()` in `src-tauri/src/config/schema.rs`, the one deliberate copy of a
 * Rust fact in the UI. A test pins it to the shared fixture; change both together.
 */
export const defaultButtonRemaps: Remap[] = [
  remap(3, "click", { type: "SymbolicHotkey", id: "MissionControl" }),
  remap(4, "click", { type: "Navigate", direction: "Back" }),
  remap(5, "click", { type: "Navigate", direction: "Forward" }),
];
