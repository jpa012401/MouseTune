import { useStatusStore } from "../store/status";

export function About() {
  const status = useStatusStore((s) => s.status);
  const os = status?.os_version;

  return (
    <>
      <h1 className="mb-1 text-xl font-semibold">MouseTune</h1>
      <p className="mb-6 text-sm text-neutral-500">
        Version {status?.app_version ?? "—"}
        {os && ` · macOS ${os.major}.${os.minor}.${os.patch}`}
      </p>
      <p className="max-w-prose text-sm">
        Inspired by Mac Mouse Fix by Noah Nuebling (github.com/noah-nuebling/mac-mouse-fix).
        MouseTune is an independent implementation.
      </p>
    </>
  );
}
