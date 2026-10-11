import { create } from "zustand";
import { commands, type Status } from "../bindings";

type StatusState = {
  status: Status | null;
  error: string | null;
  refresh: () => Promise<void>;
};

/** Mirror of backend status. Rust is the source of truth (spec §11). */
export const useStatusStore = create<StatusState>((set) => ({
  status: null,
  error: null,
  refresh: async () => {
    try {
      set({ status: await commands.getStatus(), error: null });
    } catch (err) {
      set({ error: String(err) });
    }
  },
}));
