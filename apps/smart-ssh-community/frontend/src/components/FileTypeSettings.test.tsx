// Spec 0054, Teil 5 ("Testbarkeit"): "Standardprogramme: pro Typ gesetzt,
// greift beim Öffnen, Fallback auf OS-Standard." Der "greift beim
// Öffnen"-Teil gehört zu Teil 4 (`fileTypeSettings.ts`s `appForFileName`,
// dort separat getestet) — hier geht es nur um "pro Typ gesetzt": die
// Einstellungs-UI selbst.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { open } from "@tauri-apps/plugin-dialog";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { loadFileTypeApps, saveFileTypeApps } from "../fileTypeSettings";
import { testI18n } from "../testI18n";
import { FileTypeSettings } from "./FileTypeSettings";

vi.mock("../fileTypeSettings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../fileTypeSettings")>();
  return {
    ...actual,
    loadFileTypeApps: vi.fn(),
    saveFileTypeApps: vi.fn(),
  };
});

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

function renderSettings() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <FileTypeSettings />
    </I18nextProvider>,
  );
}

describe("FileTypeSettings (Spec 0054, Teil 5)", () => {
  it("shows previously saved mappings", async () => {
    vi.mocked(loadFileTypeApps).mockResolvedValue({ ".conf": "/usr/bin/code" });

    renderSettings();

    expect(await screen.findByDisplayValue(".conf")).toBeInTheDocument();
    expect(screen.getByDisplayValue("/usr/bin/code")).toBeInTheDocument();
  });

  it("adds a row, fills it in, and saves a normalized extension", async () => {
    vi.mocked(loadFileTypeApps).mockResolvedValue({});
    vi.mocked(saveFileTypeApps).mockResolvedValue(undefined);

    renderSettings();
    await waitFor(() => expect(loadFileTypeApps).toHaveBeenCalled());

    fireEvent.click(screen.getByText("+ Zuordnung"));
    const extensionInput = screen.getByPlaceholderText(".conf");
    fireEvent.change(extensionInput, { target: { value: "LOG" } });
    const pathInput = screen.getByPlaceholderText("Pfad zum Programm");
    fireEvent.change(pathInput, { target: { value: "/usr/bin/less" } });

    fireEvent.click(screen.getByText("Speichern"));

    await waitFor(() =>
      expect(saveFileTypeApps).toHaveBeenCalledWith({ ".log": "/usr/bin/less" }),
    );
    expect(await screen.findByText("Gespeichert!")).toBeVisible();
  });

  it("drops incomplete rows (only an extension or only a path) silently on save", async () => {
    vi.mocked(loadFileTypeApps).mockResolvedValue({});
    vi.mocked(saveFileTypeApps).mockResolvedValue(undefined);

    renderSettings();
    await waitFor(() => expect(loadFileTypeApps).toHaveBeenCalled());

    fireEvent.click(screen.getByText("+ Zuordnung"));
    fireEvent.change(screen.getByPlaceholderText(".conf"), { target: { value: ".conf" } });
    // Pfad bleibt leer.

    fireEvent.click(screen.getByText("Speichern"));

    await waitFor(() => expect(saveFileTypeApps).toHaveBeenCalledWith({}));
  });

  it("removes a row without touching the others", async () => {
    vi.mocked(loadFileTypeApps).mockResolvedValue({
      ".conf": "/usr/bin/code",
      ".log": "/usr/bin/less",
    });
    vi.mocked(saveFileTypeApps).mockResolvedValue(undefined);

    renderSettings();
    await screen.findByDisplayValue(".conf");

    const removeButtons = screen.getAllByTitle("Zeile entfernen");
    fireEvent.click(removeButtons[0]);

    expect(screen.queryByDisplayValue(".conf")).not.toBeInTheDocument();
    expect(screen.getByDisplayValue(".log")).toBeInTheDocument();
  });

  it("'Durchsuchen…' fills the path field from the native file dialog", async () => {
    vi.mocked(loadFileTypeApps).mockResolvedValue({ ".conf": "" });
    vi.mocked(open).mockResolvedValue("/Applications/Code.app");

    renderSettings();
    await screen.findByDisplayValue(".conf");

    fireEvent.click(screen.getByText("Durchsuchen…"));

    expect(await screen.findByDisplayValue("/Applications/Code.app")).toBeInTheDocument();
  });

  it("shows a visible error instead of crashing when loading fails", async () => {
    vi.mocked(loadFileTypeApps).mockRejectedValue(new Error("boom"));

    renderSettings();

    expect(
      await screen.findByText("Dateityp-Zuordnungen konnten nicht geladen werden."),
    ).toBeInTheDocument();
  });
});
