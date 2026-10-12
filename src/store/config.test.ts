import { describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";
import { waitFor } from "@testing-library/react";
import { startConfigSync, useConfigStore } from "./config";
import { configView } from "../test/backend";

/** Mocks IPC by hand so the test decides when the config-changed listener is registered. */
function backendWithSlowListen() {
  const calls: string[] = [];
  let registered: (id: number) => void = () => {};
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "plugin:event|listen") return new Promise<number>((resolve) => (registered = resolve));
    if (cmd === "get_config") return configView();
    return null;
  });
  return { calls, register: () => registered(1) };
}

describe("startConfigSync", () => {
  it("loads only after it is listening for changes, so none are missed", async () => {
    const backend = backendWithSlowListen();
    const stop = startConfigSync();

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(backend.calls).toEqual(["plugin:event|listen"]);

    backend.register();
    await waitFor(() => expect(useConfigStore.getState().view).toEqual(configView()));
    expect(backend.calls).toEqual(["plugin:event|listen", "get_config"]);
    stop();
  });

  it("stopping before the listener is registered still unlistens and skips the load", async () => {
    const backend = backendWithSlowListen();
    const stop = startConfigSync();
    stop();

    backend.register();
    await waitFor(() => expect(backend.calls).toContain("plugin:event|unlisten"));
    expect(backend.calls).not.toContain("get_config");
  });
});
