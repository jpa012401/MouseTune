import { remapLabel } from "../lib/remaps";
import { useConfigStore } from "../store/config";

const listClass = "mb-6 divide-y divide-black/10 text-sm dark:divide-white/10";
const rowClass = "flex items-center justify-between py-2";
const muted = "text-neutral-500";

export function Buttons() {
  const view = useConfigStore((s) => s.view);
  const remaps = view?.config.buttons.remaps ?? [];

  return (
    <>
      <h1 className="mb-4 text-xl font-semibold">Buttons</h1>

      {view && !view.config.general.buttons_enabled && (
        <p className={`mb-4 text-sm ${muted}`}>Mouse buttons are turned off. Turn them on in General.</p>
      )}

      {view &&
        (remaps.length === 0 ? (
          <p className={`mb-6 text-sm ${muted}`}>No buttons are remapped.</p>
        ) : (
          <ul aria-label="Button remaps" className={listClass}>
            {remaps.map((remap) => {
              const label = remapLabel(remap);
              return (
                <li key={label.trigger} className={rowClass}>
                  <span>{label.trigger}</span>
                  <span>{label.action}</span>
                </li>
              );
            })}
          </ul>
        ))}

      <p className={`max-w-prose text-xs ${muted}`}>Changing these comes in a later version.</p>
    </>
  );
}
