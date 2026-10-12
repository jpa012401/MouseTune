import { create } from "zustand";
import { commands, events, type ConfigView, type General as GeneralSettings } from "../bindings";
import { configErrorMessage } from "../lib/config";

type ConfigState = {
  view: ConfigView | null;
  error: string | null;
  load: () => Promise<void>;
  setGeneral: (patch: Partial<GeneralSettings>) => Promise<void>;
  dismissRecovery: () => Promise<void>;
};

/** Mirror of the saved settings. Rust validates and persists; the UI shows what Rust returns. */
export const useConfigStore = create<ConfigState>((set, get) => ({
  view: null,
  error: null,
  load: async () => {
    try {
      set({ view: await commands.getConfig() });
    } catch (err) {
      set({ error: String(err) });
    }
  },
  setGeneral: async (patch) => {
    const { view } = get();
    if (!view) return;
    const result = await commands.setConfig({
      ...view.config,
      general: { ...view.config.general, ...patch },
    });
    if (result.status === "ok") {
      set((state) => (state.view ? { view: { ...state.view, config: result.data }, error: null } : state));
    } else {
      set({ error: configErrorMessage(result.error) });
    }
  },
  dismissRecovery: async () => {
    await commands.dismissConfigRecovery();
    set((state) => (state.view ? { view: { ...state.view, recovery: null } } : state));
  },
}));

/** Loads the config and follows changes made elsewhere (the tray). Returns a cleanup function. */
export function startConfigSync(): () => void {
  void useConfigStore.getState().load();

  const unlisteners = [
    events.configChanged.listen(({ payload }) => {
      useConfigStore.setState((state) =>
        state.view ? { view: { ...state.view, config: payload } } : state,
      );
    }),
  ];

  return () => {
    for (const unlisten of unlisteners) void unlisten.then((fn) => fn());
  };
}
