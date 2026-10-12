import { muted, rowClass } from "./rowStyles";

export function SwitchRow({
  label,
  help,
  on,
  onToggle,
  disabled,
}: {
  label: string;
  help: string;
  on: boolean;
  onToggle: () => void;
  disabled?: boolean;
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
        disabled={disabled}
        onClick={onToggle}
        className={`relative h-5 w-9 shrink-0 rounded-full transition-colors disabled:opacity-50 ${on ? "bg-green-600 dark:bg-green-500" : "bg-neutral-300 dark:bg-neutral-600"}`}
      >
        <span
          className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-white transition-transform ${on ? "translate-x-4" : ""}`}
        />
      </button>
    </li>
  );
}
