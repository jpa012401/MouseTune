import type { Action, Buttons, ClickTrigger, DragEffect, MouseButton, Remap, ScrollEffect, Timing, TriggerDuration } from "../bindings";
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

export type BindingKind = "drag" | "scroll" | "through";
export type BindingRow = { kind: "remap" | BindingKind; index: number; button: number; trigger: string; action: string };

const DIRECTIONS: ["left" | "right" | "up" | "down", string][] = [
  ["left", "Left"],
  ["right", "Right"],
  ["up", "Up"],
  ["down", "Down"],
];

export function dragLabel(effect: DragEffect): string {
  switch (effect.type) {
    case "SpacesAndMissionControl":
      return "Spaces & Mission Control";
    case "ScrollSwipe":
      return "Two-finger swipe";
    case "Discrete":
      return DIRECTIONS.filter(([d]) => effect[d])
        .map(([d, name]) => `${name}: ${actionLabel(effect[d]!)}`)
        .join(", ");
  }
}

export const SCROLL_LABELS: Record<ScrollEffect, string> = {
  Zoom: "Zoom",
  Horizontal: "Scroll horizontally",
  AppSwitcher: "App Switcher",
};
export const scrollLabel = (effect: ScrollEffect): string => SCROLL_LABELS[effect];

export const ACTS_AS_LABELS: Record<MouseButton, string> = {
  Left: "Left button",
  Right: "Right button",
  Middle: "Middle button",
  Back: "Button 4",
  Forward: "Button 5",
};
export const actsAsLabel = (button: MouseButton): string => ACTS_AS_LABELS[button];

const KIND_ORDER = { remap: 0, drag: 1, scroll: 2, through: 3 } as const;

export function bindingRows(b: Buttons): BindingRow[] {
  const rows: BindingRow[] = [
    ...b.remaps.map((r, index) => ({ kind: "remap" as const, index, button: r.trigger.button, ...remapLabel(r) })),
    ...b.drags.map((d, index) => ({ kind: "drag" as const, index, button: d.button, trigger: `${buttonName(d.button)} · Drag`, action: dragLabel(d.effect) })),
    ...b.scroll_bindings.map((s, index) => ({ kind: "scroll" as const, index, button: s.button, trigger: `${buttonName(s.button)} · Scroll`, action: scrollLabel(s.effect) })),
    ...b.hold_through.map((h, index) => ({ kind: "through" as const, index, button: h.button, trigger: `${buttonName(h.button)} · Acts as`, action: actsAsLabel(h.acts_as) })),
  ];
  // Stable: rows of the same button and kind keep their list order.
  return rows
    .map((row, order) => ({ row, order }))
    .sort((a, b) => a.row.button - b.row.button || KIND_ORDER[a.row.kind] - KIND_ORDER[b.row.kind] || a.order - b.order)
    .map(({ row }) => row);
}

export function bindingConflict(b: Buttons, button: number, kind: BindingKind | "remap"): string | null {
  const name = buttonName(button);
  if (kind !== "through" && b.hold_through.some((h) => h.button === button)) {
    return `${name} acts as another button, so it can’t have other bindings.`;
  }
  if (kind === "through") {
    const taken =
      b.remaps.some((r) => r.trigger.button === button) ||
      b.drags.some((d) => d.button === button) ||
      b.scroll_bindings.some((s) => s.button === button);
    if (taken) return `${name} already has other bindings, so it can’t act as another button.`;
  }
  const existing = bindingRows(b).find((r) => r.button === button && r.kind === kind && kind !== "remap");
  return existing ? `${existing.trigger} is already set to ${existing.action}. Change that row instead.` : null;
}

/** Mirrors Rust's full `Buttons::default()`; pinned to the fixture by a test. */
export const defaultButtons: Buttons = {
  remaps: defaultButtonRemaps,
  drags: [{ button: 3, effect: { type: "SpacesAndMissionControl" } }],
  scroll_bindings: [],
  hold_through: [],
};
