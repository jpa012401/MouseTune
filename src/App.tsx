import { useEffect, useState } from "react";
import { RouterProvider } from "react-router";
import { createAppRouter } from "./router";
import { Onboarding } from "./routes/Onboarding";
import { startStatusSync, useStatusStore } from "./store/status";

export default function App() {
  const status = useStatusStore((s) => s.status);
  const error = useStatusStore((s) => s.error);

  const [router] = useState(createAppRouter);

  useEffect(() => startStatusSync(), []);

  if (error) {
    return (
      <p role="alert" className="p-8 text-sm text-red-600">
        Couldn’t reach the MouseTune core: {error}
      </p>
    );
  }
  if (!status) {
    return <p className="p-8 text-sm text-neutral-500">Loading…</p>;
  }
  // Settings are locked until the input engine can actually work (spec §3.1).
  if (!status.accessibility_trusted) {
    return <Onboarding os={status.os_version} />;
  }
  return <RouterProvider router={router} />;
}
