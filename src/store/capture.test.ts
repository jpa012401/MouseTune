import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { emit } from "@tauri-apps/api/event";
import { act } from "@testing-library/react";
import { CAPTURE_TIMEOUT_MS, startCaptureSync, useCaptureStore } from "./capture";
import { mockBackend } from "../test/backend";

describe("useCaptureStore", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  });
  afterEach(() => {
    useCaptureStore.getState().reset();
    vi.useRealTimers();
  });

  it("capturing reports the pressed button and goes idle", async () => {
    const calls = mockBackend({ start_capture: () => null, stop_capture: () => null });
    const stop = startCaptureSync();
    await useCaptureStore.getState().startCapture();
    expect(useCaptureStore.getState().phase).toBe("capturing");
    await act(() => emit("button-captured", { button: 4 }));
    expect(useCaptureStore.getState()).toMatchObject({ phase: "idle", button: 4, timedOut: false });
    expect(calls).toContain("start_capture");
    stop();
  });

  it("capture times out after ten seconds and tells Rust to stop", async () => {
    const calls = mockBackend({ start_capture: () => null, stop_capture: () => null });
    await useCaptureStore.getState().startCapture();
    await act(async () => {
      vi.advanceTimersByTime(CAPTURE_TIMEOUT_MS);
    });
    expect(useCaptureStore.getState()).toMatchObject({ phase: "idle", timedOut: true, button: null });
    expect(calls).toContain("stop_capture");
  });

  it("recording reports the combo", async () => {
    mockBackend({ start_key_capture: () => null, stop_key_capture: () => null });
    const stop = startCaptureSync();
    await useCaptureStore.getState().startRecording();
    await act(() => emit("key-captured", { key_code: 1, modifiers: ["Command", "Shift"], key_label: "S" }));
    expect(useCaptureStore.getState()).toMatchObject({
      phase: "idle",
      combo: { key_code: 1, modifiers: ["Command", "Shift"], key_label: "S" },
    });
    stop();
  });

  it("a cancelled recording goes idle with no combo", async () => {
    mockBackend({ start_key_capture: () => null, stop_key_capture: () => null });
    const stop = startCaptureSync();
    await useCaptureStore.getState().startRecording();
    await act(() => emit("key-capture-cancelled", {}));
    expect(useCaptureStore.getState()).toMatchObject({ phase: "idle", combo: null });
    stop();
  });

  it("cancel stops whatever is running", async () => {
    const calls = mockBackend({ start_key_capture: () => null, stop_key_capture: () => null });
    await useCaptureStore.getState().startRecording();
    await useCaptureStore.getState().cancel();
    expect(useCaptureStore.getState().phase).toBe("idle");
    expect(calls).toContain("stop_key_capture");
  });

  it("a failed start shows why and stays idle", async () => {
    mockBackend({
      start_capture: () => {
        throw "input thread gone";
      },
    });
    await useCaptureStore.getState().startCapture();
    expect(useCaptureStore.getState()).toMatchObject({
      phase: "idle",
      error: "Couldn’t start listening for a button: input thread gone",
    });
  });
});
