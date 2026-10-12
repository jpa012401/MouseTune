import type { Action, DragEffect, MouseButton, ScrollEffect } from "../bindings";
import { ACTS_AS_LABELS, dragEffectReady, SCROLL_LABELS } from "../lib/remaps";
import { ActionPicker } from "./ActionPicker";

const selectClass = "rounded-md border border-black/15 bg-transparent px-2 py-1 text-sm dark:border-white/20";

const DIRECTIONS = ["left", "right", "up", "down"] as const;

export function DragEffectPicker({ value, onChange }: { value: DragEffect | null; onChange: (e: DragEffect) => void }) {
  const discrete = value?.type === "Discrete" ? value : null;
  const setDirection = (d: (typeof DIRECTIONS)[number], a: Action | null) =>
    onChange({
      type: "Discrete",
      left: discrete?.left ?? null,
      right: discrete?.right ?? null,
      up: discrete?.up ?? null,
      down: discrete?.down ?? null,
      [d]: a,
    });
  return (
    <div className="flex flex-col gap-2">
      <select
        aria-label="Drag effect"
        className={selectClass}
        value={value?.type ?? ""}
        onChange={(e) => {
          const t = e.target.value;
          if (t === "Discrete") onChange({ type: "Discrete", left: null, right: null, up: null, down: null });
          else if (t === "SpacesAndMissionControl" || t === "ScrollSwipe") onChange({ type: t });
        }}
      >
        <option value="" disabled>
          Choose a drag effect…
        </option>
        <option value="SpacesAndMissionControl">Spaces &amp; Mission Control</option>
        <option value="ScrollSwipe">Two-finger swipe</option>
        <option value="Discrete">Separate actions</option>
      </select>
      {discrete &&
        DIRECTIONS.map((d) => (
          <ActionPicker key={d} label={d[0]!.toUpperCase() + d.slice(1)} value={discrete[d]} onChange={(a) => setDirection(d, a)} />
        ))}
      {value?.type === "Discrete" && !dragEffectReady(value) && (
        <p className="text-xs text-neutral-500">Choose at least one direction.</p>
      )}
    </div>
  );
}

export function ScrollEffectPicker({ value, onChange }: { value: ScrollEffect | null; onChange: (e: ScrollEffect) => void }) {
  return (
    <select aria-label="Scroll effect" className={selectClass} value={value ?? ""} onChange={(e) => onChange(e.target.value as ScrollEffect)}>
      <option value="" disabled>
        Choose a scroll effect…
      </option>
      {Object.entries(SCROLL_LABELS).map(([v, label]) => (
        <option key={v} value={v}>
          {label}
        </option>
      ))}
    </select>
  );
}

export function ActsAsPicker({ value, onChange }: { value: MouseButton | null; onChange: (b: MouseButton) => void }) {
  return (
    <select aria-label="Acts as" className={selectClass} value={value ?? ""} onChange={(e) => onChange(e.target.value as MouseButton)}>
      <option value="" disabled>
        Choose a button…
      </option>
      {Object.entries(ACTS_AS_LABELS).map(([v, label]) => (
        <option key={v} value={v}>
          {label}
        </option>
      ))}
    </select>
  );
}
