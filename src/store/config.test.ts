import { describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";
import { waitFor } from "@testing-library/react";
import { startConfigSync, useConfigStore } from "./config";
import { config, configView, defaultRemaps, mockBackend } from "../test/backend";
import type { Config } from "../bindings";

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

describe("setRemaps", () => {
  it("saves remaps and keeps the kill switches", async () => {
    let saved: Config | undefined;
    mockBackend({
      get_config: () => configView({ config: config({ buttons_enabled: false }) }),
      set_config: (args) => (saved = (args as { config: Config }).config),
    });
    await useConfigStore.getState().load();
    expect(await useConfigStore.getState().setRemaps([])).toBe(true);
    expect(saved!.buttons.remaps).toEqual([]);
    expect(saved!.general.buttons_enabled).toBe(false);
  });

  it("a rejected remap save keeps the old list and explains why", async () => {
    mockBackend({
      set_config: () => {
        throw { kind: "InvalidRemap", index: 1, reason: "DuplicateTrigger" };
      },
    });
    await useConfigStore.getState().load();
    expect(await useConfigStore.getState().setRemaps([defaultRemaps[2]!, defaultRemaps[2]!])).toBe(false);
    expect(useConfigStore.getState().view!.config.buttons.remaps).toEqual(defaultRemaps);
    expect(useConfigStore.getState().error).toBe("Couldn’t save your settings: Button 4 · Click is set twice.");
  });
});
