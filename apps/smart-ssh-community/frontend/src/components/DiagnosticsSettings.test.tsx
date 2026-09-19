// Spec 0063, Abschnitt "Testbarkeit": "Nutzer-Vorschau vor dem
// Speichern/Teilen" — das Frontend zeigt das vom Backend bereits redigierte
// Paket erst an, speichert es nie automatisch. Diese Tests decken nur die
// UI-Seite ab (erzeugen -> Vorschau -> speichern, plus Fehlerpfade); die
// eigentliche Redaction/Feldauswahl ist Backend-Logik
// (`crates/app-shell/src/diagnostics.rs`, dort unit-getestet).
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { generateDiagnosticsBundle, openLogDirectory, saveDiagnosticsBundle } from "../api";
import { testI18n } from "../testI18n";
import { DiagnosticsSettings } from "./DiagnosticsSettings";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  openLogDirectory: vi.fn(),
  generateDiagnosticsBundle: vi.fn(),
  saveDiagnosticsBundle: vi.fn(),
}));

function renderDiagnostics() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <DiagnosticsSettings />
    </I18nextProvider>,
  );
}

describe("DiagnosticsSettings (Spec 0063)", () => {
  it("opens the log directory on click", async () => {
    vi.mocked(openLogDirectory).mockResolvedValue(undefined);

    renderDiagnostics();
    fireEvent.click(screen.getByText("Diagnose-Logs im Dateimanager öffnen"));

    await waitFor(() => expect(openLogDirectory).toHaveBeenCalled());
  });

  it("shows the generated bundle in a read-only preview, not saved automatically", async () => {
    vi.mocked(generateDiagnosticsBundle).mockResolvedValue("bundle content");

    renderDiagnostics();
    fireEvent.click(screen.getByText("Diagnosepaket erzeugen"));

    const preview = await screen.findByDisplayValue("bundle content");
    expect(preview.tagName).toBe("TEXTAREA");
    expect(preview).toHaveAttribute("readonly");
    expect(saveDiagnosticsBundle).not.toHaveBeenCalled();
  });

  it("only calls save_diagnostics_bundle after the user explicitly clicks save, with the previewed content", async () => {
    vi.mocked(generateDiagnosticsBundle).mockResolvedValue("bundle content");
    vi.mocked(saveDiagnosticsBundle).mockResolvedValue(undefined);

    renderDiagnostics();
    fireEvent.click(screen.getByText("Diagnosepaket erzeugen"));
    await screen.findByDisplayValue("bundle content");

    expect(screen.queryByText("Speichern unter …")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Speichern unter …"));

    await waitFor(() =>
      expect(saveDiagnosticsBundle).toHaveBeenCalledWith("bundle content"),
    );
  });

  it("shows a visible error instead of a preview when generation fails", async () => {
    vi.mocked(generateDiagnosticsBundle).mockRejectedValue(new Error("boom"));

    renderDiagnostics();
    fireEvent.click(screen.getByText("Diagnosepaket erzeugen"));

    expect(
      await screen.findByText("Diagnosepaket konnte nicht erzeugt werden. Error: boom"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("shows a visible error when saving fails, keeping the preview intact", async () => {
    vi.mocked(generateDiagnosticsBundle).mockResolvedValue("bundle content");
    vi.mocked(saveDiagnosticsBundle).mockRejectedValue(new Error("disk full"));

    renderDiagnostics();
    fireEvent.click(screen.getByText("Diagnosepaket erzeugen"));
    await screen.findByDisplayValue("bundle content");
    fireEvent.click(screen.getByText("Speichern unter …"));

    expect(
      await screen.findByText("Diagnosepaket konnte nicht gespeichert werden. Error: disk full"),
    ).toBeInTheDocument();
    expect(await screen.findByDisplayValue("bundle content")).toBeInTheDocument();
  });
});
