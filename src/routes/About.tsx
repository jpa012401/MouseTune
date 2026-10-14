import { useState } from "react";
import { commands } from "../bindings";
import { openErrorText } from "../lib/open";
import { useStatusStore } from "../store/status";

export function About() {
  const status = useStatusStore((s) => s.status);
  const os = status?.os_version;
  const [error, setError] = useState<string | null>(null);

  const openMacMouseFix = async () => {
    const result = await commands.openAboutLink("MacMouseFix");
    setError(result.status === "error" ? `Couldn’t open the link: ${openErrorText(result.error)}` : null);
  };

  return (
    <>
      <h1 className="mb-1 text-xl font-semibold">MouseTune</h1>
      <p className="mb-6 text-sm text-neutral-500">
        Version {status?.app_version ?? "—"}
        {os && ` · macOS ${os.major}.${os.minor}.${os.patch}`}
      </p>
      <p className="max-w-prose text-sm">
        Inspired by{" "}
        <button type="button" className="underline underline-offset-2" onClick={() => void openMacMouseFix()}>
          Mac Mouse Fix
        </button>{" "}
        by Noah Nuebling. MouseTune is an independent implementation.
      </p>
      {error && (
        <p role="alert" className="mt-2 text-sm text-red-600">
          {error}
        </p>
      )}

      <section aria-labelledby="credits-heading" className="mt-8 max-w-prose text-sm">
        <h2 id="credits-heading" className="mb-2 font-semibold">
          Credits
        </h2>
        <p className="mb-2">
          MouseTune exists because of Mac Mouse Fix by Noah Nuebling. Its smooth scrolling, button remapping and
          click-and-drag gestures for Spaces and Mission Control showed what a mouse on a Mac can feel like, and
          they were the reference for how MouseTune should behave.
        </p>
        <p className="mb-2 text-neutral-500">
          MouseTune is an independent, clean-room implementation and contains no Mac Mouse Fix source code. Mac
          Mouse Fix is © Noah Nuebling and has its own licence.
        </p>
        <button type="button" className="underline underline-offset-2" onClick={() => void openMacMouseFix()}>
          Mac Mouse Fix on GitHub
        </button>
      </section>
    </>
  );
}
