import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";
import { useConfigStore } from "../store/config";
import { useCaptureStore } from "../store/capture";
import { useLoginItemStore } from "../store/loginItem";
import { useStatusStore } from "../store/status";

afterEach(async () => {
  // Node-environment tests (scripts/) have no DOM or Tauri mocks to tear down.
  if (typeof window === "undefined") return;
  cleanup();
  // Let async unlisten() calls from unmounted components settle before the
  // mocked event internals are torn down.
  await new Promise((resolve) => setTimeout(resolve, 0));
  clearMocks();
  useStatusStore.setState({ status: null, error: null });
  useConfigStore.setState({ view: null, error: null, loadError: null });
  useLoginItemStore.setState({ status: null, pending: false, error: null });
  useCaptureStore.getState().reset();
  window.location.hash = "";
});
