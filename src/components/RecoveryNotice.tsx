import { useConfigStore } from "../store/config";

/** Says the settings file was replaced at startup, until the user dismisses it (spec §9.1). */
export function RecoveryNotice() {
  const recovery = useConfigStore((s) => s.view?.recovery);
  if (!recovery) return null;
  return (
    <div role="alert" className="mb-6 flex items-start justify-between gap-4 text-sm text-amber-700 dark:text-amber-400">
      <p>
        Your settings file couldn’t be read, so MouseTune started with default settings.{" "}
        {recovery.backup_file
          ? `The old file was kept as ${recovery.backup_file}.`
          : "The old file couldn’t be backed up and was left in place."}
      </p>
      <button
        type="button"
        onClick={() => void useConfigStore.getState().dismissRecovery()}
        className="shrink-0 underline"
      >
        Dismiss
      </button>
    </div>
  );
}
