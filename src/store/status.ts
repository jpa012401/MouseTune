import { create } from "zustand";
import { commands, events, type Status } from "../bindings";

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

/**
 * Loads status and keeps it current: backend events push permission changes,
 * and returning to the window re-reads everything. Returns a cleanup function.
 */
export function startStatusSync(): () => void {
  const { refresh } = useStatusStore.getState();
  void refresh();

  const onFocus = () => void refresh();
  window.addEventListener("focus", onFocus);

  const unlisten = events.permissionChanged.listen(({ payload }) => {
    useStatusStore.setState((state) =>
      state.status
        ? { status: { ...state.status, accessibility_trusted: payload.accessibility_trusted } }
        : state,
    );
  });

  return () => {
    window.removeEventListener("focus", onFocus);
    void unlisten.then((fn) => fn());
  };
}
