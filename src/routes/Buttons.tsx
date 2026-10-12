import { useEffect, useId, useRef, useState } from "react";
import type { Action, Buttons as ButtonSettings, DragEffect, MouseButton, Remap, ScrollEffect } from "../bindings";
import { ActionPicker } from "../components/ActionPicker";
import { AddRemapPanel } from "../components/AddRemapPanel";
import { ActsAsPicker, DragEffectPicker, ScrollEffectPicker } from "../components/BindingEffectPicker";
import { buttonClass } from "../components/buttonStyles";
import { bindingRows, dragEffectReady, DEFAULT_TIMING, defaultButtons, waitsForDoubleClick, type BindingRow } from "../lib/remaps";
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

/** What a row is being changed to: the row's own effect, held until Save. */
type Draft =
  | { kind: "remap"; value: Action | null }
  | { kind: "drag"; value: DragEffect | null }
  | { kind: "scroll"; value: ScrollEffect | null }
  | { kind: "through"; value: MouseButton | null };

const rowKey = (row: BindingRow) => `${row.kind}-${row.index}`;

function draftOf(b: ButtonSettings, row: BindingRow): Draft {
  switch (row.kind) {
    case "remap":
      return { kind: "remap", value: b.remaps[row.index]!.effect.action };
    case "drag":
      return { kind: "drag", value: b.drags[row.index]!.effect };
    case "scroll":
      return { kind: "scroll", value: b.scroll_bindings[row.index]!.effect };
    case "through":
      return { kind: "through", value: b.hold_through[row.index]!.acts_as };
  }
}

/** A recorded-but-cancelled shortcut sets a remap draft to null; the others always hold a choice. */
const draftReady = (d: Draft) => (d.kind === "drag" ? dragEffectReady(d.value) : d.value !== null);

/** Saves into the one list at `row.index`; the other lists are carried over untouched. */
function withDraft(b: ButtonSettings, row: BindingRow, d: Draft): ButtonSettings {
  const at = <T,>(list: T[], make: (old: T) => T) => list.map((x, i) => (i === row.index ? make(x) : x));
  if (d.kind === "remap" && row.kind === "remap" && d.value) {
    const action = d.value;
    return { ...b, remaps: at(b.remaps, (r) => ({ ...r, effect: { ...r.effect, action } })) };
  }
  if (d.kind === "drag" && row.kind === "drag" && d.value) {
    const effect = d.value;
    return { ...b, drags: at(b.drags, (x) => ({ ...x, effect })) };
  }
  if (d.kind === "scroll" && row.kind === "scroll" && d.value) {
    const effect = d.value;
    return { ...b, scroll_bindings: at(b.scroll_bindings, (x) => ({ ...x, effect })) };
  }
  if (d.kind === "through" && row.kind === "through" && d.value) {
    const acts_as = d.value;
    return { ...b, hold_through: at(b.hold_through, (x) => ({ ...x, acts_as })) };
  }
  return b;
}

function without(b: ButtonSettings, row: BindingRow): ButtonSettings {
  const drop = <T,>(list: T[]) => list.filter((_, i) => i !== row.index);
  switch (row.kind) {
    case "remap":
      return { ...b, remaps: drop(b.remaps) };
    case "drag":
      return { ...b, drags: drop(b.drags) };
    case "scroll":
      return { ...b, scroll_bindings: drop(b.scroll_bindings) };
    case "through":
      return { ...b, hold_through: drop(b.hold_through) };
  }
}

function DraftPicker({ draft, onChange }: { draft: Draft; onChange: (d: Draft) => void }) {
  switch (draft.kind) {
    case "remap":
      return <ActionPicker value={draft.value} onChange={(value) => onChange({ kind: "remap", value })} />;
    case "drag":
      return <DragEffectPicker value={draft.value} onChange={(value) => onChange({ kind: "drag", value })} />;
    case "scroll":
      return <ScrollEffectPicker value={draft.value} onChange={(value) => onChange({ kind: "scroll", value })} />;
    case "through":
      return <ActsAsPicker value={draft.value} onChange={(value) => onChange({ kind: "through", value })} />;
  }
}

/** What the engine uses for a hand-edited value outside 150–600 ms (it clamps when compiling). */
function clampTiming(ms: number): number {
  return Math.min(600, Math.max(150, ms));
}

/** A 150–600 ms slider that saves on release, not on every step. */
function TimingSlider({
  label,
  help,
  value,
  onCommit,
}: {
  label: string;
  help: string;
  value: number;
  onCommit: (ms: number) => Promise<boolean>;
}) {
  const id = useId();
  const shown = clampTiming(value);
  const [draft, setDraft] = useState(shown);
  useEffect(() => setDraft(shown), [shown]);
  const input = useRef<HTMLInputElement>(null);
  // The native listener is attached once, so it reads the latest props through a ref.
  const latest = useRef({ value, onCommit });
  latest.current = { value, onCommit };
  useEffect(() => {
    const el = input.current;
    if (!el) return;
    // Native `change` fires once on release, for mouse and keyboard; React's onChange is every step.
    const commit = () => {
      const ms = Number(el.value);
      const { value: saved, onCommit: save } = latest.current;
      if (ms === saved) return;
      void save(ms).then((ok) => {
        if (!ok) setDraft(clampTiming(latest.current.value));
      });
    };
    el.addEventListener("change", commit);
    return () => el.removeEventListener("change", commit);
  }, []);
  return (
    <div className="py-2 text-sm">
      <div className="flex items-center justify-between">
        <label htmlFor={id}>{label}</label>
        {/* Not <output>: its implicit status role would announce every drag step and collide with the capture prompt. */}
        <span className="tabular-nums">{draft} ms</span>
      </div>
      <input
        ref={input}
        id={id}
        type="range"
        min={150}
        max={600}
        step={10}
        value={draft}
        onChange={(e) => setDraft(Number(e.target.value))}
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
  const buttons = view?.config.buttons;
  const remaps = buttons?.remaps ?? [];
  const rows = buttons ? bindingRows(buttons) : [];
  const timing = view?.config.timing ?? DEFAULT_TIMING;
  // Capture only sees presses through the buttons tap; without it the panel could only time out.
  const canCapture = useStatusStore((s) => s.status?.input.taps.buttons ?? false);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);

  // A capture or recording must not outlive the page that started it.
  useEffect(
    () => () => {
      const capture = useCaptureStore.getState();
      if (capture.phase !== "idle") void capture.cancel();
    },
    [],
  );

  const { setButtons, setTiming } = useConfigStore.getState();

  // Adding and changing share one capture store, so only one of them is ever open.
  function startChange(row: BindingRow) {
    if (!buttons) return;
    setAdding(false);
    setEditing(rowKey(row));
    setDraft(draftOf(buttons, row));
  }

  function toggleAdding() {
    setEditing(null);
    setAdding(!adding);
  }

  async function saveChange(row: BindingRow) {
    if (!buttons || !draft) return;
    if (await setButtons(withDraft(buttons, row, draft))) setEditing(null);
  }

  async function remove(row: BindingRow) {
    if (!buttons) return;
    if (await setButtons(without(buttons, row))) setEditing(null);
  }

  async function restoreDefaults() {
    if (!window.confirm("Replace your button bindings with the defaults?")) return;
    if (await setButtons(defaultButtons)) setEditing(null);
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
          {rows.length === 0 ? (
            <p className={`mb-6 text-sm ${muted}`}>No buttons are remapped.</p>
          ) : (
            <ul aria-label="Button remaps" className={listClass}>
              {rows.map((row) => {
                const key = rowKey(row);
                return (
                  // Rows have no identity of their own; the lists are replaced on every save.
                  <li key={key}>
                    <div className={`${rowClass} gap-3`}>
                      <span>{row.trigger}</span>
                      {editing === key && draft ? (
                        <div className="flex flex-wrap items-center justify-end gap-2">
                          <DraftPicker draft={draft} onChange={setDraft} />
                          <button type="button" className={buttonClass} disabled={!draftReady(draft)} onClick={() => void saveChange(row)}>
                            Save
                          </button>
                          <button type="button" className={buttonClass} onClick={() => setEditing(null)}>
                            Cancel
                          </button>
                        </div>
                      ) : (
                        <div className="flex items-center gap-2">
                          <span className="mr-2">{row.action}</span>
                          <button type="button" className={buttonClass} onClick={() => startChange(row)}>
                            Change
                          </button>
                          <button type="button" className={buttonClass} onClick={() => void remove(row)}>
                            Delete
                          </button>
                        </div>
                      )}
                    </div>
                    {row.kind === "remap" && showsDoubleClickHint(remaps, row.index) && (
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

          {adding && <AddRemapPanel buttons={buttons!} onDone={() => setAdding(false)} />}

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
              onCommit={(hold_ms) => setTiming({ ...timing, hold_ms })}
            />
            <TimingSlider
              label="Double-click speed"
              help="How quickly a second click must follow. Longer is easier, but single clicks on buttons with double-click actions wait this long."
              value={timing.double_click_ms}
              onCommit={(double_click_ms) => setTiming({ ...timing, double_click_ms })}
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
