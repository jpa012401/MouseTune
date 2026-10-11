import { useState } from "react";
import { commands, type OsVersion } from "../bindings";
import { settingsAppName } from "../lib/os";

export function Onboarding({ os }: { os: OsVersion }) {
  const [error, setError] = useState<string | null>(null);
  const [resetDone, setResetDone] = useState(false);
  const settingsApp = settingsAppName(os);

  async function request() {
    const result = await commands.requestAccessibility();
    setError(result.status === "error" ? result.error : null);
  }

  async function reset() {
    const result = await commands.resetAccessibility();
    setError(result.status === "error" ? result.error : null);
    setResetDone(result.status === "ok");
  }

  return (
    <main
      data-tauri-drag-region
      className="mx-auto flex h-full max-w-lg flex-col justify-center gap-6 px-8 py-12"
    >
      <div>
        <h1 className="mb-2 text-2xl font-semibold">Allow MouseTune to control your mouse</h1>
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          MouseTune needs Accessibility permission to read and adjust mouse input. That’s how it
          smooths scrolling and remaps buttons. It doesn’t log what you type or send anything off
          your Mac.
        </p>
      </div>

      <ol className="list-decimal space-y-1 pl-5 text-sm">
        <li>Click <strong>Open {settingsApp}</strong>.</li>
        <li>
          Under <strong>Privacy &amp; Security → Accessibility</strong>, turn on{" "}
          <strong>MouseTune</strong>.
        </li>
        <li>Come back here. This screen continues on its own.</li>
      </ol>

      <div className="flex items-center gap-4">
        <button
          type="button"
          onClick={() => void request()}
          className="rounded-md bg-blue-500 px-4 py-2 text-sm font-medium text-white hover:bg-blue-600"
        >
          Open {settingsApp}
        </button>
        <span role="status" className="flex items-center gap-2 text-sm text-neutral-500">
          <span className="h-2 w-2 animate-pulse rounded-full bg-amber-500" aria-hidden />
          Waiting for permission…
        </span>
      </div>

      {error && (
        <p role="alert" className="text-sm text-red-600">
          Couldn’t open {settingsApp}: {error}
        </p>
      )}

      <details className="text-sm text-neutral-600 dark:text-neutral-400">
        <summary className="cursor-pointer">Already turned on, but still waiting?</summary>
        <div className="mt-2 space-y-2">
          <p>
            macOS can keep an outdated entry from an earlier version of MouseTune. Resetting removes
            it so you can grant permission again.
          </p>
          <button
            type="button"
            onClick={() => void reset()}
            className="rounded-md border border-black/15 px-3 py-1.5 hover:bg-black/5 dark:border-white/20 dark:hover:bg-white/10"
          >
            Reset permission
          </button>
          {resetDone && <p>Reset. Turn MouseTune on again in {settingsApp}.</p>}
          {import.meta.env.DEV && (
            <p className="text-amber-600">
              Development build: macOS grants this permission to the app that launched MouseTune
              (your terminal or editor). Turn it on for that app instead.
            </p>
          )}
        </div>
      </details>
    </main>
  );
}
