import { create } from "zustand";
import { commands, events, type KeyModifier } from "../bindings";

/** Matches Rust's `CAPTURE_TIMEOUT`. Rust only reports the deadline when a key arrives after it. */
export const CAPTURE_TIMEOUT_MS = 10_000;

type Combo = { key_code: number; modifiers: KeyModifier[]; key_label: string };

type CaptureState = {
  phase: "idle" | "capturing" | "recording";
  /** Last captured button, 1-based. */
  button: number | null;
  combo: Combo | null;
  timedOut: boolean;
  error: string | null;
  startCapture: () => Promise<void>;
  startRecording: () => Promise<void>;
  /** Stops whichever is running. */
  cancel: () => Promise<void>;
  reset: () => void;
};

const CLEARED = { button: null, combo: null, timedOut: false, error: null } as const;

let timer: ReturnType<typeof setTimeout> | undefined;
/** Bumped whenever a capture starts or ends, so late timers and start results can tell they are stale. */
let token = 0;

/** Ends the current capture's bookkeeping: no timer survives, and in-flight results become stale. */
function endCurrent() {
  token += 1;
  clearTimeout(timer);
  timer = undefined;
}

/** Rust's stop commands are silent, so every UI-side ending calls the matching one itself. */
function stopCommand(phase: CaptureState["phase"]) {
  return phase === "recording" ? commands.stopKeyCapture() : commands.stopCapture();
}

export const useCaptureStore = create<CaptureState>((set, get) => {
  async function begin(phase: "capturing" | "recording", start: () => Promise<void>, failure: string) {
    endCurrent();
    const mine = token;
    set({ ...CLEARED, phase });
    // Armed before awaiting, so an event or cancel during the await can retire it.
    timer = setTimeout(() => {
      if (token !== mine || get().phase !== phase) return;
      endCurrent();
      // The UI goes idle either way, and Rust's own deadline ends the capture, so a failure is not shown.
      void Promise.resolve(stopCommand(phase)).catch(() => {});
      set({ phase: "idle", timedOut: true });
    }, CAPTURE_TIMEOUT_MS);
    try {
      await start();
    } catch (err) {
      if (token !== mine) return;
      endCurrent();
      set({ phase: "idle", error: `${failure}: ${String(err)}` });
    }
  }

  return {
    phase: "idle",
    ...CLEARED,
    startCapture: () =>
      begin("capturing", () => commands.startCapture(), "Couldn’t start listening for a button"),
    startRecording: () =>
      begin("recording", () => commands.startKeyCapture(), "Couldn’t start recording a shortcut"),
    cancel: async () => {
      endCurrent();
      const { phase } = get();
      set({ phase: "idle" });
      if (phase === "idle") return;
      try {
        await stopCommand(phase);
      } catch (err) {
        set({ error: `Couldn’t stop listening: ${String(err)}` });
      }
    },
    reset: () => {
      endCurrent();
      set({ phase: "idle", ...CLEARED });
    },
  };
});

/** Follows the capture events from Rust. Returns a cleanup function. */
export function startCaptureSync(): () => void {
  const unlisteners = [
    events.buttonCaptured.listen(({ payload }) => {
      if (useCaptureStore.getState().phase !== "capturing") return;
      endCurrent();
      useCaptureStore.setState({ phase: "idle", button: payload.button, timedOut: false });
    }),
    events.keyCaptured.listen(({ payload }) => {
      if (useCaptureStore.getState().phase !== "recording") return;
      endCurrent();
      useCaptureStore.setState({
        phase: "idle",
        combo: { key_code: payload.key_code, modifiers: payload.modifiers, key_label: payload.key_label },
        timedOut: false,
      });
    }),
    events.keyCaptureCancelled.listen(() => {
      if (useCaptureStore.getState().phase !== "recording") return;
      endCurrent();
      useCaptureStore.setState({ phase: "idle", combo: null });
    }),
  ];
  return () => {
    for (const unlisten of unlisteners) void unlisten.then((fn) => fn()).catch(() => {});
  };
}
