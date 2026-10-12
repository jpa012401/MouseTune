import { describe, expect, it } from "vitest";
import { act, render, screen } from "@testing-library/react";
import { ActionPicker } from "./ActionPicker";
import { useCaptureStore } from "../store/capture";
import { mockBackend } from "../test/backend";

describe("ActionPicker", () => {
  it("doesn't show the error of a button capture it didn't start", async () => {
    mockBackend({ start_capture: () => { throw "tap unavailable"; } });
    const combo = { type: "KeyCombo", key_code: 21, modifiers: ["Command"], key_label: "4" } as const;
    render(<ActionPicker value={{ ...combo, modifiers: [...combo.modifiers] }} onChange={() => {}} />);
    expect(screen.getByRole("button", { name: "Record again" })).toBeTruthy();
    await act(() => useCaptureStore.getState().startCapture());
    expect(useCaptureStore.getState().error).toBe("Couldn’t start listening for a button: tap unavailable");
    expect(screen.queryByText("Couldn’t start listening for a button: tap unavailable")).toBeNull();
  });

  it("shows why its own recording couldn't start", async () => {
    mockBackend({ start_key_capture: () => { throw "tap unavailable"; } });
    const combo = { type: "KeyCombo", key_code: 21, modifiers: ["Command"], key_label: "4" } as const;
    render(<ActionPicker value={{ ...combo, modifiers: [...combo.modifiers] }} onChange={() => {}} />);
    await act(async () => screen.getByRole("button", { name: "Record again" }).click());
    expect(await screen.findByText("Couldn’t start recording a shortcut: tap unavailable")).toBeTruthy();
  });
});
