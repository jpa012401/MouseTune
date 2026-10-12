import { useEffect, useRef, useState } from "react";
import type { Action } from "../bindings";
import { ACTION_GROUPS, actionLabel, actionValue } from "../lib/actions";
import { useCaptureStore } from "../store/capture";
import { buttonClass } from "./buttonStyles";

const KEYS = "keys";

type Props = {
  value: Action | null;
  /** `null` while a shortcut is being recorded, so a stale choice can't be saved. */
  onChange: (action: Action | null) => void;
  label?: string;
};

/** Chooses a built-in action, or records a keyboard shortcut through the capture store. */
export function ActionPicker({ value, onChange, label = "Action" }: Props) {
  const phase = useCaptureStore((s) => s.phase);
  const combo = useCaptureStore((s) => s.combo);
  const error = useCaptureStore((s) => s.error);
  const [keysChosen, setKeysChosen] = useState(value?.type === "KeyCombo");
  // True only while a recording this picker started is running, so another picker's recording is ignored.
  const [listening, setListening] = useState(false);
  const listeningRef = useRef(false);
  listeningRef.current = listening;

  useEffect(() => {
    if (!listening || phase === "recording") return;
    setListening(false);
    if (combo) onChange({ type: "KeyCombo", ...combo });
  }, [listening, phase, combo, onChange]);

  useEffect(
    () => () => {
      const capture = useCaptureStore.getState();
      if (listeningRef.current && capture.phase === "recording") void capture.cancel();
    },
    [],
  );

  function record() {
    void useCaptureStore.getState().startRecording();
    setListening(true);
  }

  function choose(selected: string) {
    if (selected === KEYS) {
      setKeysChosen(true);
      onChange(null);
      record();
      return;
    }
    if (listening) {
      setListening(false);
      void useCaptureStore.getState().cancel();
    }
    setKeysChosen(false);
    const choice = ACTION_GROUPS.flatMap((g) => g.choices).find((c) => c.value === selected);
    onChange(choice ? choice.action : null);
  }

  const selected = keysChosen ? KEYS : value ? actionValue(value) : "";

  return (
    <div className="flex flex-wrap items-center gap-2">
      <select
        aria-label={label}
        value={selected}
        onChange={(e) => choose(e.target.value)}
        className="rounded-md border border-black/15 bg-transparent px-2 py-1 text-sm dark:border-white/20"
      >
        <option value="" disabled>
          Choose an action…
        </option>
        {ACTION_GROUPS.map((group) => (
          <optgroup key={group.label} label={group.label}>
            {group.choices.map((choice) => (
              <option key={choice.value} value={choice.value}>
                {choice.label}
              </option>
            ))}
          </optgroup>
        ))}
        <option value={KEYS}>Keyboard shortcut…</option>
      </select>

      {keysChosen &&
        (listening ? (
          <span role="status" className="text-sm text-neutral-500">
            Press a shortcut… (Esc to cancel)
          </span>
        ) : (
          <>
            {value?.type === "KeyCombo" && <kbd className="text-sm">{actionLabel(value)}</kbd>}
            <button type="button" className={buttonClass} onClick={record}>
              Record again
            </button>
            {error && (
              <span role="alert" className="text-sm text-red-600">
                {error}
              </span>
            )}
          </>
        ))}
    </div>
  );
}
