import { useEffect } from "react";
import type { Device } from "../bindings";
import { startConfigSync, useConfigStore } from "../store/config";
import { useStatusStore } from "../store/status";

const POLL_MS = 1000;

const listClass = "mb-6 divide-y divide-black/10 text-sm dark:divide-white/10";
const rowClass = "flex items-center justify-between py-2";
const muted = "text-neutral-500";

function TapRow({ label, on }: { label: string; on: boolean }) {
  return (
    <li className={rowClass}>
      <span>{label}</span>
      <span className={on ? "text-green-600 dark:text-green-400" : muted}>{on ? "On" : "Off"}</span>
    </li>
  );
}

function SwitchRow({
  label,
  help,
  on,
  onToggle,
}: {
  label: string;
  help: string;
  on: boolean;
  onToggle: () => void;
}) {
  return (
    <li className={rowClass}>
      <div>
        <div>{label}</div>
        <div className={`${muted} text-xs`}>{help}</div>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={on}
        aria-label={label}
        onClick={onToggle}
        className={`relative h-5 w-9 shrink-0 rounded-full transition-colors ${on ? "bg-green-600 dark:bg-green-500" : "bg-neutral-300 dark:bg-neutral-600"}`}
      >
        <span
          className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-white transition-transform ${on ? "translate-x-4" : ""}`}
        />
      </button>
    </li>
  );
}

function StatRow({ label, value }: { label: string; value: number }) {
  return (
    <li className={rowClass}>
      <span>{label}</span>
      <span className="tabular-nums">{value}</span>
    </li>
  );
}

function deviceDetails({ manufacturer, button_count, has_wheel }: Device) {
  const buttons = `${button_count} ${button_count === 1 ? "button" : "buttons"}`;
  return [manufacturer, buttons, has_wheel ? "scroll wheel" : ""]
    .filter(Boolean)
    .join(" · ");
}

export function General() {
  const input = useStatusStore((s) => s.status?.input);
  const view = useConfigStore((s) => s.view);
  const saveError = useConfigStore((s) => s.error);
  const { setGeneral, dismissRecovery } = useConfigStore.getState();

  useEffect(() => startConfigSync(), []);

  // The engine pushes taps and devices as events, but scroll counters change
  // on every wheel tick, so they are polled instead.
  useEffect(() => {
    const id = setInterval(() => void useStatusStore.getState().refresh(), POLL_MS);
    return () => clearInterval(id);
  }, []);

  return (
    <>
      <h1 className="mb-4 text-xl font-semibold">General</h1>

      {view?.recovery && (
        <div role="alert" className="mb-6 flex items-start justify-between gap-4 text-sm text-amber-700 dark:text-amber-400">
          <p>
            Your settings file couldn’t be read, so MouseTune started with default settings.{" "}
            {view.recovery.backup_file
              ? `The old file was kept as ${view.recovery.backup_file}.`
              : "The old file couldn’t be backed up and was left in place."}
          </p>
          <button type="button" onClick={() => void dismissRecovery()} className="shrink-0 underline">
            Dismiss
          </button>
        </div>
      )}

      {input?.degraded && (
        <p role="alert" className="mb-6 text-sm text-red-600">
          MouseTune recovered from an internal error. Some input may have been passed through
          unchanged. Check the logs for details.
        </p>
      )}

      {view && (
        <>
          <h2 className="mb-2 text-sm font-semibold">Input</h2>
          <ul aria-label="Input" className="divide-y divide-black/10 text-sm dark:divide-white/10">
            <SwitchRow
              label="Mouse buttons"
              help="Turn off to let macOS handle extra buttons normally."
              on={view.config.general.buttons_enabled}
              onToggle={() => void setGeneral({ buttons_enabled: !view.config.general.buttons_enabled })}
            />
            <SwitchRow
              label="Scrolling"
              help="Turn off to let macOS handle the scroll wheel normally."
              on={view.config.general.scroll_enabled}
              onToggle={() => void setGeneral({ scroll_enabled: !view.config.general.scroll_enabled })}
            />
          </ul>
          {saveError && (
            <p role="alert" className="mt-2 text-sm text-red-600">
              {saveError}
            </p>
          )}
          <div className="mb-6" />
        </>
      )}

      <h2 className="mb-2 text-sm font-semibold">Connected mice</h2>
      {input && input.devices.length > 0 ? (
        <ul aria-label="Connected mice" className={listClass}>
          {input.devices.map((d) => (
            <li key={d.id} className="py-2">
              <div>{d.name}</div>
              <div className={`${muted} text-xs`}>{deviceDetails(d)}</div>
            </li>
          ))}
        </ul>
      ) : (
        <p className={`mb-6 text-sm ${muted}`}>No mouse connected</p>
      )}

      <h2 className="mb-2 text-sm font-semibold">Input engine</h2>
      <ul aria-label="Input engine" className={listClass}>
        <TapRow label="Buttons" on={input?.taps.buttons ?? false} />
        <TapRow label="Scroll" on={input?.taps.scroll ?? false} />
        <TapRow label="Modifier keys" on={input?.taps.flags ?? false} />
      </ul>

      <h2 className="mb-2 text-sm font-semibold">Scroll events</h2>
      <ul aria-label="Scroll events" className="divide-y divide-black/10 text-sm dark:divide-white/10">
        <StatRow label="Mouse wheel" value={input?.scroll_stats.mouse_wheel ?? 0} />
        <StatRow label="Passed through" value={input?.scroll_stats.passed_through ?? 0} />
      </ul>
      <p className={`mt-2 max-w-prose text-xs ${muted}`}>
        Mouse wheel ticks are what MouseTune will smooth. Trackpad and Magic Mouse scrolling should
        only ever count as passed through.
      </p>
    </>
  );
}
