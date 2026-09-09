// Spec 0052, Abschnitt 6 ("Testbarkeit"): "Über-Dialog: zeigt Version +
// Hash, Text ist selektierbar/kopierbar."
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { getAppInfo } from "../api";
import { testI18n } from "../testI18n";
import { AboutSettings } from "./AboutSettings";

vi.mock("../api", () => ({
  getAppInfo: vi.fn(),
}));

function renderAbout() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <AboutSettings />
    </I18nextProvider>,
  );
}

describe("AboutSettings (Spec 0052)", () => {
  it("shows version, commit hash, and edition in the shared display format", async () => {
    vi.mocked(getAppInfo).mockResolvedValue({
      version: "0.4.1",
      commitHash: "a5b3e01",
      versionDisplay: "0.4.1 (a5b3e01)",
      edition: "Community",
    });

    renderAbout();

    expect(await screen.findByText("0.4.1 (a5b3e01) · Community")).toBeInTheDocument();
  });

  it("copies the version display text to the clipboard on click", async () => {
    vi.mocked(getAppInfo).mockResolvedValue({
      version: "0.4.1",
      commitHash: "a5b3e01",
      versionDisplay: "0.4.1 (a5b3e01)",
      edition: "Community",
    });
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });

    renderAbout();
    const button = await screen.findByText("0.4.1 (a5b3e01) · Community");
    fireEvent.click(button);

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("0.4.1 (a5b3e01)"));
    expect(await screen.findByText("Kopiert!")).toBeInTheDocument();
  });

  it("shows a load-error message instead of crashing when get_app_info fails", async () => {
    vi.mocked(getAppInfo).mockRejectedValue(new Error("boom"));

    renderAbout();

    expect(
      await screen.findByText("Versions-/Build-Information konnte nicht geladen werden."),
    ).toBeInTheDocument();
  });
});
