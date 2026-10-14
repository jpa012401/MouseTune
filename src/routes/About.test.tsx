import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "../App";
import { mockBackend, status } from "../test/backend";

function renderAbout() {
  window.location.hash = "#/about";
  return render(<App />);
}

describe("About page", () => {
  it("opens Mac Mouse Fix's page", async () => {
    let sent: unknown;
    mockBackend({
      get_status: () => status(),
      open_about_link: (args) => {
        sent = args;
        return null;
      },
    });
    renderAbout();
    fireEvent.click(await screen.findByRole("button", { name: "Mac Mouse Fix" }));
    await waitFor(() => expect(sent).toEqual({ link: "MacMouseFix" }));
  });

  it("says when the link couldn't be opened", async () => {
    mockBackend({
      get_status: () => status(),
      open_about_link: () => {
        throw { kind: "Refused", target: "https://github.com/noah-nuebling/mac-mouse-fix" };
      },
    });
    renderAbout();
    fireEvent.click(await screen.findByRole("button", { name: "Mac Mouse Fix" }));
    expect(
      await screen.findByText("Couldn’t open the link: macOS refused to open https://github.com/noah-nuebling/mac-mouse-fix"),
    ).toBeTruthy();
  });
});
