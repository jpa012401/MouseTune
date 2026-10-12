import { useEffect, useState } from "react";
import type { Action, Buttons, ClickTrigger, DragEffect, MouseButton, ScrollEffect } from "../bindings";
import { bindingConflict, buttonName, findTrigger, remapLabel, type BindingKind } from "../lib/remaps";
import { CAPTURE_TIMEOUT_MS, useCaptureStore } from "../store/capture";
import { useConfigStore } from "../store/config";
import { ActionPicker } from "./ActionPicker";
import { ActsAsPicker, DragEffectPicker, ScrollEffectPicker } from "./BindingEffectPicker";
import { buttonClass } from "./buttonStyles";

type TriggerChoice =
  | { label: string; kind: "remap"; level: number; duration: ClickTrigger["duration"] }
  | { label: string; kind: BindingKind };

const TRIGGERS: TriggerChoice[] = [
  { label: "Click", kind: "remap", level: 1, duration: "click" },
  { label: "Double click", kind: "remap", level: 2, duration: "click" },
  { label: "Triple click", kind: "remap", level: 3, duration: "click" },
  { label: "Hold", kind: "remap", level: 1, duration: "hold" },
  { label: "Drag", kind: "drag" },
  { label: "Scroll", kind: "scroll" },
  { label: "Acts as", kind: "through" },
];

const SECONDS = CAPTURE_TIMEOUT_MS / 1000;

/** Seconds left of the capture deadline, ticking once a second while `running`. */
function useCountdown(running: boolean): number {
  const [left, setLeft] = useState(SECONDS);
  useEffect(() => {
    if (!running) return;
    const started = Date.now();
    setLeft(SECONDS);
    const id = setInterval(() => {
      setLeft(Math.max(0, Math.ceil((CAPTURE_TIMEOUT_MS - (Date.now() - started)) / 1000)));
    }, 1000);
    return () => clearInterval(id);
  }, [running]);
  return left;
}

/** Adds one remap: capture a button, pick a trigger and an action, then save. */
export function AddRemapPanel({ buttons, onDone }: { buttons: Buttons; onDone: () => void }) {
  const phase = useCaptureStore((s) => s.phase);
  const captured = useCaptureStore((s) => s.button);
  const timedOut = useCaptureStore((s) => s.timedOut);
  const error = useCaptureStore((s) => s.error);
  // Kept locally: recording a shortcut starts a new capture session, which clears the store's button.
  const [button, setButton] = useState<number | null>(null);
  const [triggerIndex, setTriggerIndex] = useState<number | null>(null);
  const [action, setAction] = useState<Action | null>(null);
  const [dragEffect, setDragEffect] = useState<DragEffect | null>(null);
  const [scrollEffect, setScrollEffect] = useState<ScrollEffect | null>(null);
  const [actsAs, setActsAs] = useState<MouseButton | null>(null);
  const secondsLeft = useCountdown(phase === "capturing" && button === null);

  useEffect(() => {
    void useCaptureStore.getState().startCapture();
    return () => {
      const capture = useCaptureStore.getState();
      if (capture.phase !== "idle") void capture.cancel();
      capture.reset();
    };
  }, []);

  useEffect(() => {
    if (button === null && captured !== null) setButton(captured);
  }, [button, captured]);

  const choice = triggerIndex === null ? null : TRIGGERS[triggerIndex]!;
  const trigger: ClickTrigger | null =
    button !== null && choice?.kind === "remap" ? { button, level: choice.level, duration: choice.duration } : null;
  const { remaps } = buttons;
  const conflict = trigger ? findTrigger(remaps, trigger) : -1;
  const conflictLabel = conflict >= 0 ? remapLabel(remaps[conflict]!) : null;
  // Remaps keep their own exact-trigger check; the others share `bindingConflict`.
  const bindingProblem =
    conflictLabel
      ? `${conflictLabel.trigger} is already set to ${conflictLabel.action}. Change that row instead.`
      : button !== null && choice
        ? bindingConflict(buttons, button, choice.kind)
        : null;
  const effectChosen =
    choice?.kind === "remap" ? action !== null
    : choice?.kind === "drag" ? dragEffect !== null
    : choice?.kind === "scroll" ? scrollEffect !== null
    : choice?.kind === "through" ? actsAs !== null
    : false;
  const canAdd = effectChosen && bindingProblem === null;

  function cancel() {
    void useCaptureStore.getState().cancel();
    onDone();
  }

  async function add() {
    if (button === null || !choice || !canAdd) return;
    let next: Buttons | null = null;
    if (choice.kind === "remap" && trigger && action) next = { ...buttons, remaps: [...remaps, { trigger, effect: { action } }] };
    else if (choice.kind === "drag" && dragEffect) next = { ...buttons, drags: [...buttons.drags, { button, effect: dragEffect }] };
    else if (choice.kind === "scroll" && scrollEffect) next = { ...buttons, scroll_bindings: [...buttons.scroll_bindings, { button, effect: scrollEffect }] };
    else if (choice.kind === "through" && actsAs) next = { ...buttons, hold_through: [...buttons.hold_through, { button, acts_as: actsAs }] };
    if (next && (await useConfigStore.getState().setButtons(next))) onDone();
  }

  const cancelButton = (
    <button type="button" className={buttonClass} onClick={cancel}>
      Cancel
    </button>
  );

  if (button === null) {
    const failed = phase === "idle" && (timedOut || error !== null);
    return (
      <section aria-label="Add remap" className="mb-6 rounded-md border border-black/10 p-4 text-sm dark:border-white/10">
        {failed ? (
          <>
            <p role="alert" className="mb-3">
              {timedOut ? "No button pressed. Try again." : error}
            </p>
            <div className="flex gap-2">
              <button type="button" className={buttonClass} onClick={() => void useCaptureStore.getState().startCapture()}>
                Try again
              </button>
              {cancelButton}
            </div>
          </>
        ) : (
          <>
            <p role="status" className="mb-1">Press the mouse button you want to remap…</p>
            <p className="mb-3 text-xs text-neutral-500">Left and right buttons can’t be remapped.</p>
            <div className="flex items-center gap-3">
              <span aria-live="off" className="tabular-nums text-neutral-500">{`${secondsLeft} s`}</span>
              {cancelButton}
            </div>
          </>
        )}
      </section>
    );
  }

  return (
    <section aria-label="Add remap" className="mb-6 rounded-md border border-black/10 p-4 text-sm dark:border-white/10">
      <fieldset className="mb-3">
        <legend className="mb-2 font-medium">{buttonName(button)}</legend>
        <div className="flex flex-wrap gap-4">
          {TRIGGERS.map((t, i) => (
            <label key={t.label} className="flex items-center gap-1">
              <input type="radio" name="trigger" checked={triggerIndex === i} onChange={() => setTriggerIndex(i)} />
              {t.label}
            </label>
          ))}
        </div>
      </fieldset>

      {bindingProblem && (
        <p role="alert" className="mb-3 text-red-600">
          {bindingProblem}
        </p>
      )}

      <div className="mb-3">
        {(!choice || choice.kind === "remap") && <ActionPicker value={action} onChange={setAction} />}
        {choice?.kind === "drag" && <DragEffectPicker value={dragEffect} onChange={setDragEffect} />}
        {choice?.kind === "scroll" && <ScrollEffectPicker value={scrollEffect} onChange={setScrollEffect} />}
        {choice?.kind === "through" && <ActsAsPicker value={actsAs} onChange={setActsAs} />}
      </div>

      <div className="flex gap-2">
        <button type="button" className={buttonClass} disabled={!canAdd} onClick={() => void add()}>
          Add
        </button>
        {cancelButton}
      </div>
    </section>
  );
}
