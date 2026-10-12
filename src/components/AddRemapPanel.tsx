import { useEffect, useState } from "react";
import type { Action, ClickTrigger, Remap } from "../bindings";
import { buttonName, findTrigger, remapLabel } from "../lib/remaps";
import { CAPTURE_TIMEOUT_MS, useCaptureStore } from "../store/capture";
import { useConfigStore } from "../store/config";
import { ActionPicker } from "./ActionPicker";
import { buttonClass } from "./buttonStyles";

const TRIGGERS: { label: string; level: number; duration: ClickTrigger["duration"] }[] = [
  { label: "Click", level: 1, duration: "click" },
  { label: "Double click", level: 2, duration: "click" },
  { label: "Triple click", level: 3, duration: "click" },
  { label: "Hold", level: 1, duration: "hold" },
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
export function AddRemapPanel({ remaps, onDone }: { remaps: Remap[]; onDone: () => void }) {
  const phase = useCaptureStore((s) => s.phase);
  const captured = useCaptureStore((s) => s.button);
  const timedOut = useCaptureStore((s) => s.timedOut);
  const error = useCaptureStore((s) => s.error);
  // Kept locally: recording a shortcut starts a new capture session, which clears the store's button.
  const [button, setButton] = useState<number | null>(null);
  const [triggerIndex, setTriggerIndex] = useState<number | null>(null);
  const [action, setAction] = useState<Action | null>(null);
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
    button !== null && choice ? { button, level: choice.level, duration: choice.duration } : null;
  const conflict = trigger ? findTrigger(remaps, trigger) : -1;
  const conflictLabel = conflict >= 0 ? remapLabel(remaps[conflict]!) : null;
  const canAdd = trigger !== null && action !== null && conflict < 0;

  function cancel() {
    void useCaptureStore.getState().cancel();
    onDone();
  }

  async function add() {
    if (!trigger || !action) return;
    if (await useConfigStore.getState().setRemaps([...remaps, { trigger, effect: { action } }])) onDone();
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
            <p className="mb-1">Press the mouse button you want to remap…</p>
            <p className="mb-3 text-xs text-neutral-500">Left and right buttons can’t be remapped.</p>
            <div className="flex items-center gap-3">
              <span aria-live="polite" className="tabular-nums text-neutral-500">{`${secondsLeft} s`}</span>
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

      {conflictLabel && (
        <p role="alert" className="mb-3 text-red-600">
          {`${conflictLabel.trigger} is already set to ${conflictLabel.action}. Change that row instead.`}
        </p>
      )}

      <div className="mb-3">
        <ActionPicker value={action} onChange={setAction} />
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
