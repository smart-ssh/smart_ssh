// Spec 0063, Abschnitt "Testbarkeit": "Nutzer-Vorschau vor dem
// Speichern/Teilen" — das Frontend zeigt das vom Backend bereits redigierte
// Paket erst an, speichert es nie automatisch. Diese Tests decken nur die
// UI-Seite ab (erzeugen -> Vorschau -> speichern, plus Fehlerpfade); die
// eigentliche Redaction/Feldauswahl ist Backend-Logik
// (`crates/app-shell/src/diagnostics.rs`, dort unit-getestet).
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import {
  generateDiagnosticsBundle,
  getKeychainStatus,
  openLogDirectory,
  saveDiagnosticsBundle,
} from "../api";
import { testI18n } from "../testI18n";
import { DiagnosticsSettings } from "./DiagnosticsSettings";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  openLogDirectory: vi.fn(),
  generateDiagnosticsBundle: vi.fn(),
  saveDiagnosticsBundle: vi.fn(),
  getKeychainStatus: vi.fn(),
}));

function renderDiagnostics() {
  // Voreinstellung für die Tests, die den Schlüsselbund nicht selbst
  // setzen — sonst bliebe die `useEffect`-Promise unaufgelöst.
  if (vi.mocked(getKeychainStatus).mock.results.length === 0) {
    vi.mocked(getKeychainStatus).mockResolvedValue({ available: true, reason: null });
  }
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

// Spec 0071, A15: Der Startdialog verschwindet, sobald er bestätigt wurde —
// ohne diese Zeile gäbe es danach keinen Ort mehr, an dem ein Nutzer den
// Schlüsselbund-Zustand und den nächsten Schritt nachschlagen kann.
describe("DiagnosticsSettings — Schlüsselbund-Zustand (Spec 0071, A15)", () => {
  it("zeigt 'verfügbar', wenn der Schlüsselbund erreichbar war", async () => {
    vi.mocked(getKeychainStatus).mockResolvedValue({ available: true, reason: null });

    renderDiagnostics();

    const row = await screen.findByTestId("keychain-status");
    await waitFor(() => expect(row).toHaveTextContent("Systemschlüsselbund: verfügbar"));
    expect(row).not.toHaveTextContent("apt install");
  });

  it("nennt bei fehlendem Anbieter das Paket und die blockierten Funktionen", async () => {
    vi.mocked(getKeychainStatus).mockResolvedValue({
      available: false,
      reason: "no_secret_service_provider",
    });

    renderDiagnostics();

    const row = await screen.findByTestId("keychain-status");
    await waitFor(() => expect(row).toHaveTextContent("nicht verfügbar"));
    expect(row).toHaveTextContent("gnome-keyring");
    expect(row).toHaveTextContent("KeePassXC");
    expect(row).toHaveTextContent("API-Key");
  });

  // X3: ein gesperrter Schlüsselbund ist kein fehlendes Paket — auch hier
  // nicht, sonst installiert ein KDE-Nutzer `gnome-keyring` neben sein
  // laufendes KWallet.
  it("schickt bei einem gesperrten Schlüsselbund nicht zu einer Paketinstallation", async () => {
    vi.mocked(getKeychainStatus).mockResolvedValue({ available: false, reason: "locked" });

    renderDiagnostics();

    const row = await screen.findByTestId("keychain-status");
    await waitFor(() => expect(row).toHaveTextContent("gesperrt"));
    expect(row).not.toHaveTextContent("apt");
    expect(row).not.toHaveTextContent("install");
  });

  // A4: Ein künftiger, dem Frontend unbekannter Grund darf keine leere
  // Zeile ergeben.
  it("fällt bei einem unbekannten Grund auf den vollständigen Auffangtext zurück", async () => {
    vi.mocked(getKeychainStatus).mockResolvedValue({
      available: false,
      // Absichtlich ein Wert, den `KeychainUnavailableReason` (noch) nicht
      // kennt — simuliert ein neueres Backend.
      reason: "something_new_from_a_future_backend" as never,
    });

    renderDiagnostics();

    const row = await screen.findByTestId("keychain-status");
    await waitFor(() => expect(row).toHaveTextContent("nicht verfügbar"));
    expect(row).toHaveTextContent("Ursache unbekannt");
  });
});
