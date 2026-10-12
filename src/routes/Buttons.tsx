import { useEffect, useState } from "react";
import type { Action, Remap } from "../bindings";
import { ActionPicker } from "../components/ActionPicker";
import { AddRemapPanel } from "../components/AddRemapPanel";
import { buttonClass } from "../components/buttonStyles";
import { defaultButtonRemaps, remapLabel, waitsForDoubleClick } from "../lib/remaps";
import { useCaptureStore } from "../store/capture";
import { useConfigStore } from "../store/config";

const listClass = "mb-6 divide-y divide-black/10 text-sm dark:divide-white/10";
const rowClass = "flex items-center justify-between py-2";
const muted = "text-neutral-500";

/** The hint goes under the first row of each button whose single clicks wait for a possible double click. */
function showsDoubleClickHint(remaps: Remap[], index: number): boolean {
  const { button } = remaps[index]!.trigger;
  return remaps.findIndex((r) => r.trigger.button === button) === index && waitsForDoubleClick(remaps, button);
}

export function Buttons() {
  const view = useConfigStore((s) => s.view);
  const saveError = useConfigStore((s) => s.error);
  const loadError = useConfigStore((s) => s.loadError);
  const remaps = view?.config.buttons.remaps ?? [];
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

  const { setRemaps } = useConfigStore.getState();

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
                      <p className={`pb-2 text-xs ${muted}`}>Single clicks on this button will wait up to 260 ms.</p>
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

          <div className="flex gap-2">
            <button type="button" className={buttonClass} aria-expanded={adding} onClick={toggleAdding}>
              + Add remap
            </button>
            <button type="button" className={buttonClass} onClick={() => void restoreDefaults()}>
              Restore defaults
            </button>
          </div>
        </>
      )}
    </>
  );
}
