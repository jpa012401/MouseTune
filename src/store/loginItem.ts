import { create } from "zustand";
import { commands, type LoginItemStatus } from "../bindings";
import { loginItemErrorMessage } from "../lib/loginItem";

type LoginItemState = {
  /** What macOS has; null until read. */
  status: LoginItemStatus | null;
  /** A change is in flight; the switch is disabled so a second click can't race it. */
  pending: boolean;
  error: string | null;
  load: () => Promise<void>;
  set: (enabled: boolean) => Promise<void>;
};

/** Mirror of macOS's login item. Never stored in the config (Phase 7a design §2). */
export const useLoginItemStore = create<LoginItemState>((set, get) => ({
  status: null,
  pending: false,
  error: null,
  load: async () => {
    try {
      set({ status: await commands.getLaunchAtLogin() });
    } catch (err) {
      set({ error: `Couldn’t read launch at login: ${String(err)}` });
    }
  },
  set: async (enabled) => {
    set({ pending: true });
    try {
      const result = await commands.setLaunchAtLogin(enabled);
      if (result.status === "ok") {
        set({ status: result.data, error: null });
      } else {
        set({ error: loginItemErrorMessage(result.error, enabled) });
        // Show what macOS actually has, not what was asked.
        await get().load();
      }
    } catch (err) {
      set({ error: loginItemErrorMessage(err, enabled) });
      await get().load();
    } finally {
      set({ pending: false });
    }
  },
}));
