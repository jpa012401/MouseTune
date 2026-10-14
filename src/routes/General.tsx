import { useEffect, useState } from "react";
import { commands, type Device } from "../bindings";
import { buttonClass } from "../components/buttonStyles";
import { listClass, muted, rowClass } from "../components/rowStyles";
import { SwitchRow } from "../components/SwitchRow";
import { launchAtLoginHelp, launchAtLoginOn } from "../lib/loginItem";
import { openErrorText } from "../lib/open";
import { useConfigStore } from "../store/config";
import { useLoginItemStore } from "../store/loginItem";
import { useStatusStore } from "../store/status";

const POLL_MS = 1000;

function TapRow({ label, on }: { label: string; on: boolean }) {
  return (
    <li className={rowClass}>
      <span>{label}</span>
      <span className={on ? "text-green-600 dark:text-green-400" : muted}>{on ? "On" : "Off"}</span>
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
  const loadError = useConfigStore((s) => s.loadError);
  const { setGeneral } = useConfigStore.getState();
  const loginStatus = useLoginItemStore((s) => s.status);
  const loginPending = useLoginItemStore((s) => s.pending);
  const loginError = useLoginItemStore((s) => s.error);
  const [openLogsError, setOpenLogsError] = useState<string | null>(null);

  const openLogs = async () => {
    const result = await commands.openLogsFolder();
    setOpenLogsError(result.status === "error" ? `Couldn’t open the logs folder: ${openErrorText(result.error)}` : null);
  };

  useEffect(() => {
    const load = () => void useLoginItemStore.getState().load();
    load();
    // System Settings can change the login item while the window is in the background.
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, []);

  // The engine pushes taps and devices as events, but scroll counters change
  // on every wheel tick, so they are polled instead.
  useEffect(() => {
    const id = setInterval(() => void useStatusStore.getState().refresh(), POLL_MS);
    return () => clearInterval(id);
  }, []);

  return (
    <>
      <h1 className="mb-4 text-xl font-semibold">General</h1>

      {input?.degraded && (
        <p role="alert" className="mb-6 text-sm text-red-600">
          MouseTune recovered from an internal error. Some input may have been passed through
          unchanged. Check the logs for details.
        </p>
      )}

      {loadError && (
        <p role="alert" className="mb-6 text-sm text-red-600">
          {loadError}
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
            <SwitchRow
              label="Freeze pointer while dragging"
              help="Keeps the pointer still during drag gestures, like a trackpad."
              on={view.config.general.freeze_pointer_during_drag}
              onToggle={() => void setGeneral({ freeze_pointer_during_drag: !view.config.general.freeze_pointer_during_drag })}
            />
          </ul>
          {saveError && (
            <p role="alert" className="mt-2 text-sm text-red-600">
              {saveError}
            </p>
          )}
          <div className="mb-6" />
          <h2 className="mb-2 text-sm font-semibold">Startup</h2>
          <ul aria-label="Startup" className="divide-y divide-black/10 text-sm dark:divide-white/10">
            <SwitchRow
              label="Launch at login"
              help={launchAtLoginHelp(loginStatus ?? "Off")}
              on={loginStatus !== null && launchAtLoginOn(loginStatus)}
              disabled={loginStatus === null || loginStatus === "Unavailable" || loginPending}
              onToggle={() => void useLoginItemStore.getState().set(!(loginStatus !== null && launchAtLoginOn(loginStatus)))}
            />
            <SwitchRow
              label="Show menu bar item"
              help={
                view.config.general.show_menu_bar_item
                  ? "MouseTune’s status and switches in the menu bar."
                  : "Open MouseTune again from Applications or Spotlight to get back here."
              }
              on={view.config.general.show_menu_bar_item}
              onToggle={() => void setGeneral({ show_menu_bar_item: !view.config.general.show_menu_bar_item })}
            />
          </ul>
          {loginError && (
            <p role="alert" className="mt-2 text-sm text-red-600">
              {loginError}
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
        <TapRow label="Drag" on={input?.taps.drag ?? false} />
      </ul>

      <h2 className="mb-2 text-sm font-semibold">Scroll events</h2>
      <ul aria-label="Scroll events" className="divide-y divide-black/10 text-sm dark:divide-white/10">
        <StatRow label="Mouse wheel" value={input?.scroll_stats.mouse_wheel ?? 0} />
        <StatRow label="Passed through" value={input?.scroll_stats.passed_through ?? 0} />
      </ul>
      <p className={`mt-2 max-w-prose text-xs ${muted}`}>
        Mouse wheel ticks are what MouseTune smooths. Trackpad and Magic Mouse scrolling should
        only ever count as passed through.
      </p>

      <h2 className="mb-2 mt-6 text-sm font-semibold">Button actions</h2>
      <ul aria-label="Button actions" className="divide-y divide-black/10 text-sm dark:divide-white/10">
        <StatRow label="Performed" value={input?.actions.performed ?? 0} />
        <StatRow label="Dropped" value={input?.actions.dropped ?? 0} />
        <StatRow label="Failed" value={input?.actions.failed ?? 0} />
      </ul>
      <p className={`mt-2 max-w-prose text-xs ${muted}`}>
        Dropped means MouseTune was too busy and skipped an action. Failed means macOS refused it;
        check the logs.
      </p>

      <h2 className="mb-2 mt-6 text-sm font-semibold">Troubleshooting</h2>
      <button type="button" className={buttonClass} onClick={() => void openLogs()}>
        Open logs folder
      </button>
      {openLogsError && (
        <p role="alert" className="mt-2 text-sm text-red-600">
          {openLogsError}
        </p>
      )}
    </>
  );
}
