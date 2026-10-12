import { useEffect, useId, useState } from "react";
import type { Action, Remap } from "../bindings";
import { ActionPicker } from "../components/ActionPicker";
import { AddRemapPanel } from "../components/AddRemapPanel";
import { buttonClass } from "../components/buttonStyles";
import { DEFAULT_TIMING, defaultButtonRemaps, remapLabel, waitsForDoubleClick } from "../lib/remaps";
import { useCaptureStore } from "../store/capture";
import { useConfigStore } from "../store/config";
import { useStatusStore } from "../store/status";

const listClass = "mb-6 divide-y divide-black/10 text-sm dark:divide-white/10";
const rowClass = "flex items-center justify-between py-2";
const muted = "text-neutral-500";

/** The hint goes under the first row of each button whose single clicks wait for a possible double click. */
function showsDoubleClickHint(remaps: Remap[], index: number): boolean {
  const { button } = remaps[index]!.trigger;
  return remaps.findIndex((r) => r.trigger.button === button) === index && waitsForDoubleClick(remaps, button);
}

/** A 150–600 ms slider that saves on release, not on every step. */
function TimingSlider({ label, help, value, onCommit }: { label: string; help: string; value: number; onCommit: (ms: number) => void }) {
  const id = useId();
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft !== value) onCommit(draft);
  };
  return (
    <div className="py-2 text-sm">
      <div className="flex items-center justify-between">
        <label htmlFor={id}>{label}</label>
        {/* Not <output>: its implicit status role would announce every drag step and collide with the capture prompt. */}
        <span className="tabular-nums">{draft} ms</span>
      </div>
      <input
        id={id}
        type="range"
        min={150}
        max={600}
        step={10}
        value={draft}
        onChange={(e) => setDraft(Number(e.target.value))}
        onPointerUp={commit}
        onKeyUp={commit}
        onBlur={commit}
        className="w-full"
      />
      <p className={`text-xs ${muted}`}>{help}</p>
    </div>
  );
}

export function Buttons() {
  const view = useConfigStore((s) => s.view);
  const saveError = useConfigStore((s) => s.error);
  const loadError = useConfigStore((s) => s.loadError);
  const remaps = view?.config.buttons.remaps ?? [];
  const timing = view?.config.timing ?? DEFAULT_TIMING;
  // Capture only sees presses through the buttons tap; without it the panel could only time out.
  const canCapture = useStatusStore((s) => s.status?.input.taps.buttons ?? false);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState<Action | null>(null);

  // A capture or recording must not outlive the page that started it.
  useEffect(
    () => () => {
      const capture = useCaptureStore.getState();
      if (capture.phase !== "idle") void capture.cancel();
    },
    [],
  );

  const { setRemaps, setTiming } = useConfigStore.getState();

  // Adding and changing share one capture store, so only one of them is ever open.
  function startChange(index: number) {
    setAdding(false);
    setEditing(index);
    setDraft(remaps[index]!.effect.action);
  }

  function toggleAdding() {
    setEditing(null);
    setAdding(!adding);
  }

  async function saveChange(index: number) {
    if (!draft) return;
    const next = remaps.map((r, i) => (i === index ? { ...r, effect: { ...r.effect, action: draft } } : r));
    if (await setRemaps(next)) setEditing(null);
  }

  async function remove(index: number) {
    if (await setRemaps(remaps.filter((_, i) => i !== index))) setEditing(null);
  }

  async function restoreDefaults() {
    if (!window.confirm("Replace your button remaps with the defaults?")) return;
    if (await setRemaps(defaultButtonRemaps)) setEditing(null);
  }

  return (
    <>
      <h1 className="mb-4 text-xl font-semibold">Buttons</h1>

      {loadError && (
        <p role="alert" className="mb-6 text-sm text-red-600">
          {loadError}
        </p>
      )}

      {view && !view.config.general.buttons_enabled && (
        <p className={`mb-4 text-sm ${muted}`}>Mouse buttons are turned off. Turn them on in General.</p>
      )}

      {view && (
        <>
          {remaps.length === 0 ? (
            <p className={`mb-6 text-sm ${muted}`}>No buttons are remapped.</p>
          ) : (
            <ul aria-label="Button remaps" className={listClass}>
              {remaps.map((remap, index) => {
                const label = remapLabel(remap);
                return (
                  // Rows have no identity of their own; the list is replaced on every save.
                  <li key={index}>
                    <div className={`${rowClass} gap-3`}>
                      <span>{label.trigger}</span>
                      {editing === index ? (
                        <div className="flex flex-wrap items-center justify-end gap-2">
                          <ActionPicker value={draft} onChange={setDraft} />
                          <button type="button" className={buttonClass} disabled={!draft} onClick={() => void saveChange(index)}>
                            Save
                          </button>
                          <button type="button" className={buttonClass} onClick={() => setEditing(null)}>
                            Cancel
                          </button>
                        </div>
                      ) : (
                        <div className="flex items-center gap-2">
                          <span className="mr-2">{label.action}</span>
                          <button type="button" className={buttonClass} onClick={() => startChange(index)}>
                            Change
                          </button>
                          <button type="button" className={buttonClass} onClick={() => void remove(index)}>
                            Delete
                          </button>
                        </div>
                      )}
                    </div>
                    {showsDoubleClickHint(remaps, index) && (
                      <p className={`pb-2 text-xs ${muted}`}>{`Single clicks on this button will wait up to ${timing.double_click_ms} ms.`}</p>
                    )}
                  </li>
                );
              })}
            </ul>
          )}

          {saveError && (
            <p role="alert" className="mb-4 text-sm text-red-600">
              {saveError}
            </p>
          )}

          {adding && <AddRemapPanel remaps={remaps} onDone={() => setAdding(false)} />}

          <div className="flex flex-wrap gap-2">
            {canCapture ? (
              <button type="button" className={buttonClass} aria-expanded={adding} onClick={toggleAdding}>
                + Add remap
              </button>
            ) : (
              <p className={`self-center text-sm ${muted}`}>Turn on mouse buttons in General to add a remap.</p>
            )}
            <button type="button" className={buttonClass} onClick={() => void restoreDefaults()}>
              Restore defaults
            </button>
          </div>

          <h2 className="mb-2 mt-8 text-sm font-semibold">Timing</h2>
          <div className="mb-4 divide-y divide-black/10 dark:divide-white/10">
            <TimingSlider
              label="Hold delay"
              help="How long to hold a button before a Hold action fires."
              value={timing.hold_ms}
              onCommit={(hold_ms) => void setTiming({ ...timing, hold_ms })}
            />
            <TimingSlider
              label="Double-click speed"
              help="How quickly a second click must follow. Longer is easier, but single clicks on buttons with double-click actions wait this long."
              value={timing.double_click_ms}
              onCommit={(double_click_ms) => void setTiming({ ...timing, double_click_ms })}
            />
          </div>
          <button type="button" className={buttonClass} onClick={() => void setTiming(DEFAULT_TIMING)}>
            Restore timing defaults
          </button>
        </>
      )}
    </>
  );
}
