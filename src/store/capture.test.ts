import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { emit } from "@tauri-apps/api/event";
import { act } from "@testing-library/react";
import { startCaptureSync as startSync, useCaptureStore } from "./capture";
import { mockBackend } from "../test/backend";

const syncs: Array<() => void> = [];
/** Every sync a test starts is stopped in afterEach, even when the test fails. */
function startCaptureSync() {
  const stop = startSync();
  syncs.push(stop);
  return stop;
}

describe("useCaptureStore", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  });
  afterEach(() => {
    for (const stop of syncs.splice(0)) stop();
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
      vi.advanceTimersByTime(10_000);
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

  it("a button captured before the deadline is not overwritten by a late timeout", async () => {
    let started: () => void = () => {};
    const calls = mockBackend({
      start_capture: () => new Promise((resolve) => (started = () => resolve(null))),
      stop_capture: () => null,
    });
    startCaptureSync();
    const starting = useCaptureStore.getState().startCapture();
    await act(() => emit("button-captured", { button: 4 }));
    started();
    await starting;
    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });
    expect(useCaptureStore.getState()).toMatchObject({ phase: "idle", button: 4, timedOut: false });
    expect(calls).not.toContain("stop_capture");
  });

  it("a cancelled capture does not time out later", async () => {
    let started: () => void = () => {};
    const calls = mockBackend({
      start_capture: () => new Promise((resolve) => (started = () => resolve(null))),
      stop_capture: () => null,
    });
    const starting = useCaptureStore.getState().startCapture();
    await useCaptureStore.getState().cancel();
    started();
    await starting;
    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });
    expect(calls.filter((c) => c === "stop_capture")).toHaveLength(1);
    expect(useCaptureStore.getState().timedOut).toBe(false);
  });

  it("starting twice leaves exactly one timeout", async () => {
    const calls = mockBackend({ start_capture: () => null, stop_capture: () => null });
    await Promise.all([useCaptureStore.getState().startCapture(), useCaptureStore.getState().startCapture()]);
    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });
    expect(calls.filter((c) => c === "stop_capture")).toHaveLength(1);
    expect(useCaptureStore.getState()).toMatchObject({ phase: "idle", timedOut: true });
  });

  it("a recording times out and tells Rust to stop", async () => {
    const calls = mockBackend({ start_key_capture: () => null, stop_key_capture: () => null });
    await useCaptureStore.getState().startRecording();
    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });
    expect(useCaptureStore.getState()).toMatchObject({ phase: "idle", timedOut: true });
    expect(calls).toContain("stop_key_capture");
  });

  it("a button event on an idle store changes nothing", async () => {
    mockBackend({});
    startCaptureSync();
    await act(() => emit("button-captured", { button: 4 }));
    expect(useCaptureStore.getState()).toMatchObject({ phase: "idle", button: null });
  });

  it("a key event while capturing a button is ignored and the timer stays alive", async () => {
    const calls = mockBackend({ start_capture: () => null, stop_capture: () => null });
    startCaptureSync();
    await useCaptureStore.getState().startCapture();
    await act(() => emit("key-captured", { key_code: 1, modifiers: [], key_label: "S" }));
    expect(useCaptureStore.getState()).toMatchObject({ phase: "capturing", combo: null });
    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });
    expect(calls).toContain("stop_capture");
    expect(useCaptureStore.getState().timedOut).toBe(true);
  });

  it("a failed cancel says so and still goes idle", async () => {
    mockBackend({
      start_capture: () => null,
      stop_capture: () => {
        throw "gone";
      },
    });
    await useCaptureStore.getState().startCapture();
    await useCaptureStore.getState().cancel();
    expect(useCaptureStore.getState()).toMatchObject({ phase: "idle", error: "Couldn’t stop listening: gone" });
  });

  it("a start that fails after a cancel does not clobber the store", async () => {
    let fail: (e: unknown) => void = () => {};
    mockBackend({
      start_capture: () => new Promise((_, reject) => (fail = reject)),
      stop_capture: () => null,
    });
    const starting = useCaptureStore.getState().startCapture();
    await useCaptureStore.getState().cancel();
    fail("late");
    await starting;
    expect(useCaptureStore.getState().error).toBeNull();
  });

  it("starting a recording while capturing a button stops the button capture", async () => {
    const calls = mockBackend({ start_capture: () => null, stop_capture: () => null, start_key_capture: () => null, stop_key_capture: () => null });
    await useCaptureStore.getState().startCapture();
    await useCaptureStore.getState().startRecording();
    expect(calls).toEqual(["start_capture", "stop_capture", "start_key_capture"]);
    expect(useCaptureStore.getState().phase).toBe("recording");
  });

  it("starting a button capture while recording stops the recording, even if the stop fails", async () => {
    const calls = mockBackend({
      start_capture: () => null,
      start_key_capture: () => null,
      stop_key_capture: () => {
        throw "gone";
      },
    });
    await useCaptureStore.getState().startRecording();
    await useCaptureStore.getState().startCapture();
    await act(async () => {});
    expect(calls).toEqual(["start_key_capture", "stop_key_capture", "start_capture"]);
    expect(useCaptureStore.getState()).toMatchObject({ phase: "capturing", error: null });
  });
});
