import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";
import { useStatusStore } from "../store/status";

afterEach(async () => {
  cleanup();
  // Let async unlisten() calls from unmounted components settle before the
  // mocked event internals are torn down.
  await new Promise((resolve) => setTimeout(resolve, 0));
  clearMocks();
  useStatusStore.setState({ status: null, error: null });
  window.location.hash = "";
});
