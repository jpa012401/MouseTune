import { create } from "zustand";
import { commands, events, type ConfigView, type General as GeneralSettings, type Buttons, type Config, type Remap, type Scroll as ScrollSettings, type Timing } from "../bindings";
import { configErrorMessage } from "../lib/config";

type ConfigState = {
  view: ConfigView | null;
  /** Why the last save or dismiss failed. Cleared by the next success or a change from elsewhere. */
  error: string | null;
  /** Why the settings couldn't be loaded at all. */
  loadError: string | null;
  load: () => Promise<void>;
  setGeneral: (patch: Partial<GeneralSettings>) => Promise<void>;
  setScroll: (patch: Partial<ScrollSettings>) => Promise<void>;
  /** True when saved; otherwise `error` says why and the old list stays. */
  setRemaps: (remaps: Remap[]) => Promise<boolean>;
  /** Saves all four binding lists; callers pass the whole `Buttons` so no list is dropped. */
  setButtons: (buttons: Buttons) => Promise<boolean>;
  setTiming: (timing: Timing) => Promise<boolean>;
  dismissRecovery: () => Promise<void>;
};

type SetState = (partial: Partial<ConfigState>) => void;

/** The one save path. Errors name rows in the list being saved, not the stored one. */
async function save(set: SetState, next: Config): Promise<boolean> {
  try {
    const result = await commands.setConfig(next);
    if (result.status === "error") {
      set({ error: configErrorMessage(result.error, next.buttons.remaps) });
      return false;
    }
    // A save can clear the recovery notice in Rust (the unreadable file was overwritten).
    const { recovery } = await commands.getConfig();
    set({ view: { config: result.data, recovery }, error: null });
    return true;
  } catch (err) {
    set({ error: configErrorMessage(err, next.buttons.remaps) });
    return false;
  }
}

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
    await save(set, { ...view.config, general: { ...view.config.general, ...patch } });
  },
  setScroll: async (patch) => {
    const { view } = get();
    if (!view) return;
    await save(set, { ...view.config, scroll: { ...view.config.scroll, ...patch } });
  },
  setButtons: async (buttons) => {
    const { view } = get();
    if (!view) return false;
    return save(set, { ...view.config, buttons });
  },
  setRemaps: async (remaps) => {
    const { view } = get();
    if (!view) return false;
    return get().setButtons({ ...view.config.buttons, remaps });
  },
  setTiming: async (timing) => {
    const { view } = get();
    if (!view) return false;
    return save(set, { ...view.config, timing });
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
