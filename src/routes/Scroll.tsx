import { useEffect, useId, useState } from "react";
import {
  commands,
  type CurvePreview as CurvePreviewData,
  type KeyModifier,
  type Scroll as ScrollSettings,
  type ScrollModifiers,
  type ScrollSpeed,
  type Smoothness,
} from "../bindings";
import { CurvePreview } from "../components/CurvePreview";
import { listClass, muted, rowClass } from "../components/rowStyles";
import { SwitchRow } from "../components/SwitchRow";
import { MODIFIER_KEYS, modifierKeyLabel } from "../lib/scroll";
import { useConfigStore } from "../store/config";

const heading = "mb-2 text-sm font-semibold";

const SMOOTHNESS: [Smoothness, string][] = [
  ["low", "Low"],
  ["regular", "Regular"],
  ["high", "High"],
];
const SPEEDS: [ScrollSpeed, string][] = [
  ["system", "System"],
  ["low", "Low"],
  ["medium", "Medium"],
  ["high", "High"],
];
const EFFECTS: [keyof ScrollModifiers, string][] = [
  ["horizontal", "Scroll horizontally"],
  ["precise", "Scroll precisely"],
  ["swift", "Scroll faster"],
];

function Segmented<T extends string>({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  value: T;
  options: [T, string][];
  disabled: boolean;
  onChange: (value: T) => void;
}) {
  const id = useId();
  return (
    <li className={rowClass}>
      <span id={id}>{label}</span>
      <div role="radiogroup" aria-labelledby={id} className="flex overflow-hidden rounded-md border border-black/15 dark:border-white/20">
        {options.map(([option, text]) => (
          <button
            key={option}
            type="button"
            role="radio"
            aria-checked={value === option}
            disabled={disabled}
            onClick={() => onChange(option)}
            className={`px-2.5 py-1 text-xs disabled:opacity-50 ${value === option ? "bg-blue-600 text-white" : "hover:bg-black/5 dark:hover:bg-white/10"}`}
          >
            {text}
          </button>
        ))}
      </div>
    </li>
  );
}

function KeySelect({
  label,
  value,
  taken,
  disabled,
  onChange,
}: {
  label: string;
  value: KeyModifier | null;
  /** Keys bound to the other effects. */
  taken: KeyModifier[];
  disabled: boolean;
  onChange: (key: KeyModifier | null) => void;
}) {
  const id = useId();
  return (
    <li className={rowClass}>
      <label htmlFor={id}>{label}</label>
      <select
        id={id}
        value={value ?? ""}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value === "" ? null : (e.target.value as KeyModifier))}
        className="rounded-md border border-black/15 bg-transparent px-2 py-1 text-sm disabled:opacity-50 dark:border-white/20"
      >
        <option value="">None</option>
        {MODIFIER_KEYS.map((key) => (
          <option key={key} value={key} disabled={taken.includes(key)}>
            {modifierKeyLabel(key)}
          </option>
        ))}
      </select>
    </li>
  );
}

/** The plot for the current curve inputs; null while smoothing is off or the call failed. */
function useCurvePreview(scroll: ScrollSettings | undefined): CurvePreviewData | null {
  const [preview, setPreview] = useState<CurvePreviewData | null>(null);
  const smooth = scroll?.smooth ?? false;
  const smoothness = scroll?.smoothness;
  const speed = scroll?.speed;
  const precise = scroll?.precise;
  useEffect(() => {
    if (!scroll || !smooth) {
      setPreview(null);
      return;
    }
    let current = true;
    commands.scrollCurvePreview(scroll).then(
      (next) => current && setPreview(next),
      () => current && setPreview(null),
    );
    return () => {
      current = false;
    };
    // Only what shapes the curve: other edits don't re-plot.
  }, [smooth, smoothness, speed, precise]);
  return preview;
}

const TEST_LINES = Array.from({ length: 40 }, (_, i) => `Scroll test line ${i + 1}`);

export function Scroll() {
  const view = useConfigStore((s) => s.view);
  const saveError = useConfigStore((s) => s.error);
  const loadError = useConfigStore((s) => s.loadError);
  const { setScroll } = useConfigStore.getState();
  const scroll = view?.config.scroll;
  const preview = useCurvePreview(scroll);

  if (!view || !scroll) {
    return (
      <>
        <h1 className="mb-4 text-xl font-semibold">Scroll</h1>
        {loadError && (
          <p role="alert" className="mb-6 text-sm text-red-600">
            {loadError}
          </p>
        )}
      </>
    );
  }

  const scrollingOn = view.config.general.scroll_enabled;
  const smoothOn = scrollingOn && scroll.smooth;
  const keys = scroll.keyboard_modifiers;
  const taken = (effect: keyof ScrollModifiers) =>
    EFFECTS.filter(([other]) => other !== effect)
      .map(([other]) => keys[other])
      .filter((key): key is KeyModifier => key !== null);

  return (
    <>
      <h1 className="mb-4 text-xl font-semibold">Scroll</h1>

      {saveError && (
        <p role="alert" className="mb-6 text-sm text-red-600">
          {saveError}
        </p>
      )}

      {!scrollingOn && <p className="mb-6 text-sm">Scrolling is off. Turn it on in General or the menu bar.</p>}

      <h2 className={heading}>Smoothing</h2>
      <ul aria-label="Smoothing" className={listClass}>
        <SwitchRow
          label="Smooth scrolling"
          help="Animate each wheel tick, like a trackpad."
          on={scroll.smooth}
          disabled={!scrollingOn}
          onToggle={() => void setScroll({ smooth: !scroll.smooth })}
        />
        <Segmented
          label="Smoothness"
          value={scroll.smoothness}
          options={SMOOTHNESS}
          disabled={!smoothOn}
          onChange={(smoothness) => void setScroll({ smoothness })}
        />
        <Segmented label="Speed" value={scroll.speed} options={SPEEDS} disabled={!smoothOn} onChange={(speed) => void setScroll({ speed })} />
        <SwitchRow
          label="Trackpad-like momentum"
          help="Keep coasting after the last tick, as a trackpad does."
          on={scroll.trackpad_simulation}
          disabled={!smoothOn}
          onToggle={() => void setScroll({ trackpad_simulation: !scroll.trackpad_simulation })}
        />
        <SwitchRow
          label="Precise when scrolling slowly"
          help="A slow wheel moves a pixel or two per tick."
          on={scroll.precise}
          disabled={!smoothOn}
          onToggle={() => void setScroll({ precise: !scroll.precise })}
        />
      </ul>

      {scroll.smooth && preview && <CurvePreview preview={preview} />}

      <h2 className={heading}>Direction</h2>
      <ul aria-label="Direction" className={listClass}>
        <SwitchRow
          label="Invert vertical"
          help="Flip up and down."
          on={scroll.invert_vertical}
          disabled={!scrollingOn}
          onToggle={() => void setScroll({ invert_vertical: !scroll.invert_vertical })}
        />
        <SwitchRow
          label="Invert horizontal"
          help="Flip left and right."
          on={scroll.invert_horizontal}
          disabled={!scrollingOn}
          onToggle={() => void setScroll({ invert_horizontal: !scroll.invert_horizontal })}
        />
      </ul>

      <h2 className={heading}>Modifier keys</h2>
      <ul aria-label="Modifier keys" className="divide-y divide-black/10 text-sm dark:divide-white/10">
        {EFFECTS.map(([effect, label]) => (
          <KeySelect
            key={effect}
            label={label}
            value={keys[effect]}
            taken={taken(effect)}
            disabled={!smoothOn}
            onChange={(key) => void setScroll({ keyboard_modifiers: { ...keys, [effect]: key } })}
          />
        ))}
      </ul>
      <p className={`${muted} mb-6 mt-2 text-xs`}>
        {smoothOn
          ? "Keys you don’t assign pass through, so ⌃ + scroll still zooms the screen if Accessibility Zoom is on."
          : "Modifiers need smooth scrolling."}
      </p>

      <h2 className={heading}>Try it here</h2>
      <div
        role="region"
        tabIndex={0}
        aria-label="Scroll test area"
        className="mb-6 h-40 overflow-auto rounded-md border border-black/15 p-3 text-sm dark:border-white/20"
      >
        {TEST_LINES.map((line) => (
          <p key={line} className="py-1">
            {line}
          </p>
        ))}
      </div>
    </>
  );
}
