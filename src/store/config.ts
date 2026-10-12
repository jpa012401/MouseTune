import { create } from "zustand";
import { commands, events, type ConfigView, type General as GeneralSettings } from "../bindings";
import { configErrorMessage } from "../lib/config";

type ConfigState = {
  view: ConfigView | null;
  /** Why the last save or dismiss failed. Cleared by the next success or a change from elsewhere. */
  error: string | null;
  /** Why the settings couldn't be loaded at all. */
  loadError: string | null;
  load: () => Promise<void>;
  setGeneral: (patch: Partial<GeneralSettings>) => Promise<void>;
  dismissRecovery: () => Promise<void>;
};

/** Mirror of the saved settings. Rust validates and persists; the UI shows what Rust returns. */
export const useConfigStore = create<ConfigState>((set, get) => ({
  view: null,
  error: null,
  loadError: null,
  load: async () => {
    try {
      set({ view: await commands.getConfig(), loadError: null });
    } catch (err) {
      set({ loadError: `Couldn’t load your settings: ${String(err)}` });
    }
  },
  setGeneral: async (patch) => {
    const { view } = get();
    if (!view) return;
    try {
      const result = await commands.setConfig({
        ...view.config,
        general: { ...view.config.general, ...patch },
      });
      if (result.status === "error") {
        set({ error: configErrorMessage(result.error) });
        return;
      }
      // A save can clear the recovery notice in Rust (the unreadable file was overwritten).
      const { recovery } = await commands.getConfig();
      set({ view: { config: result.data, recovery }, error: null });
    } catch (err) {
      set({ error: configErrorMessage(err) });
    }
  },
  dismissRecovery: async () => {
    try {
      await commands.dismissConfigRecovery();
      set((state) => (state.view ? { view: { ...state.view, recovery: null }, error: null } : state));
    } catch (err) {
      set({ error: `Couldn’t dismiss the notice: ${String(err)}` });
    }
  },
}));

/** Follows changes made elsewhere (the tray), then loads the config. Returns a cleanup function. */
export function startConfigSync(): () => void {
  let stopped = false;
  const unlisten = events.configChanged.listen(({ payload }) => {
    useConfigStore.setState((state) =>
      state.view ? { view: { ...state.view, config: payload }, error: null } : state,
    );
  });
  // Load only once the listener is registered, so a change made in between isn't missed.
  const loadIfRunning = () => {
    if (!stopped) void useConfigStore.getState().load();
  };
  unlisten.then(loadIfRunning, loadIfRunning);

  return () => {
    stopped = true;
    void unlisten.then((fn) => fn());
  };
}
