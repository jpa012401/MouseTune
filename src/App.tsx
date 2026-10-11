import { useEffect, useState } from "react";
import { useStatusStore } from "./store/status";

const SECTIONS = [
  { id: "buttons", label: "Buttons", phase: "Phase 4" },
  { id: "scroll", label: "Scroll", phase: "Phase 5" },
  { id: "general", label: "General", phase: "Phase 7" },
  { id: "about", label: "About", phase: "Phase 7" },
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

export default function App() {
  const { status, error, refresh } = useStatusStore();
  const [section, setSection] = useState<SectionId>("buttons");

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const current = SECTIONS.find((s) => s.id === section)!;
  const os = status?.os_version;
  const settingsAppName = os && os.major < 13 ? "System Preferences" : "System Settings";

  return (
    <div className="flex h-full">
      <nav className="flex w-48 shrink-0 flex-col gap-1 border-r border-black/10 bg-black/[0.03] px-3 pb-3 pt-10 dark:border-white/10 dark:bg-white/[0.03]">
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            type="button"
            onClick={() => setSection(s.id)}
            aria-current={s.id === section ? "page" : undefined}
            className="rounded-md px-3 py-1.5 text-left text-sm aria-[current=page]:bg-blue-500 aria-[current=page]:text-white"
          >
            {s.label}
          </button>
        ))}
      </nav>

      <main className="flex-1 overflow-y-auto px-8 pb-8 pt-10">
        <section aria-label="Status" className="mb-8 rounded-lg border border-black/10 p-4 text-sm dark:border-white/10">
          {error ? (
            <p role="alert" className="text-red-600">
              Couldn’t reach the MouseTune core: {error}
            </p>
          ) : !status ? (
            <p className="text-neutral-500">Loading…</p>
          ) : (
            <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1">
              <dt className="text-neutral-500">Accessibility</dt>
              <dd>
                {status.accessibility_trusted
                  ? "Granted"
                  : `Not granted. Enable MouseTune in ${settingsAppName} → Privacy & Security → Accessibility.`}
              </dd>
              <dt className="text-neutral-500">macOS</dt>
              <dd>
                {os!.major}.{os!.minor}.{os!.patch}
              </dd>
              <dt className="text-neutral-500">Version</dt>
              <dd>{status.app_version}</dd>
            </dl>
          )}
        </section>

        <h1 className="mb-2 text-xl font-semibold">{current.label}</h1>
        <p className="text-sm text-neutral-500">Coming in {current.phase} of the roadmap.</p>
      </main>
    </div>
  );
}
