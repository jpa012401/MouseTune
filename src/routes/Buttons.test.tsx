import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import App from "../App";
import { config, configView, mockBackend, status } from "../test/backend";

function renderButtons() {
  window.location.hash = "#/buttons";
  return render(<App />);
}

describe("Buttons page", () => {
  it("lists the saved remaps", async () => {
    mockBackend({ get_status: () => status() });
    renderButtons();
    const list = await screen.findByRole("list", { name: "Button remaps" });
    const rows = within(list).getAllByRole("listitem").map((li) => li.textContent);
    expect(rows).toEqual([
      expect.stringContaining("Mission Control"),
      expect.stringContaining("Show Desktop"),
      expect.stringContaining("Back"),
      expect.stringContaining("Forward"),
    ]);
    expect(rows[0]).toContain("Middle button · Click");
  });

  it("says so when there are no remaps", async () => {
    mockBackend({ get_status: () => status(), get_config: () => configView({ config: { ...config(), buttons: { remaps: [] } } }) });
    renderButtons();
    expect(await screen.findByText("No buttons are remapped.")).toBeTruthy();
  });

  it("explains that buttons are off when the kill switch is off", async () => {
    mockBackend({ get_status: () => status(), get_config: () => configView({ config: config({ buttons_enabled: false }) }) });
    renderButtons();
    expect(await screen.findByText("Mouse buttons are turned off. Turn them on in General.")).toBeTruthy();
  });
});
