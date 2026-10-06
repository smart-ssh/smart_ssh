// Issue #16: Anzeige der Datenpfade in den Einstellungen. Die Pfade selbst
// kommen aus dem Backend (`get_data_paths`, dort getestet, inkl.
// `SMART_SSH_DATA_DIR`); hier nur die UI: Beschriftung, Kopieren, „Ordner
// öffnen" mit Bezeichner statt Pfad, Sitzungen, Fehlerpfade.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getDataPaths, openDataPathFolder } from "../api";
import { testI18n } from "../testI18n";
import type { DataPathEntryDto } from "../types";
import { DataPathsSection } from "./DataPathsSection";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  getDataPaths: vi.fn(),
  openDataPathFolder: vi.fn(),
}));

const ENTRIES: DataPathEntryDto[] = [
  { id: "database", label: null, path: "/data/smart-ssh.db", isDirectory: false },
  { id: "logs", label: null, path: "/logs/Smart SSH", isDirectory: true },
  { id: "hostKeys", label: null, path: "/data/host_keys.json", isDirectory: false },
  { id: "wrappingFile", label: null, path: "/data/smart-ssh.db.master-key", isDirectory: false },
  { id: "mcpSettings", label: null, path: "/app/settings.json", isDirectory: false },
];

const writeText = vi.fn();

beforeEach(() => {
  vi.mocked(getDataPaths).mockReset();
  vi.mocked(openDataPathFolder).mockReset();
  writeText.mockReset();
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
});

function renderSection() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <DataPathsSection />
    </I18nextProvider>,
  );
}

describe("DataPathsSection (Issue #16)", () => {
  it("zeigt alle eingebauten Pfade mit übersetzter Beschriftung und Sitzungen als 'in der Datenbank'", async () => {
    vi.mocked(getDataPaths).mockResolvedValue(ENTRIES);

    renderSection();

    expect(await screen.findByText("/data/smart-ssh.db")).toBeInTheDocument();
    expect(screen.getByTestId("data-path-database")).toHaveTextContent("Datenbank");
    expect(screen.getByTestId("data-path-logs")).toHaveTextContent("Logs");
    expect(screen.getByTestId("data-path-logs")).toHaveTextContent("/logs/Smart SSH");
    expect(screen.getByTestId("data-path-hostKeys")).toHaveTextContent("Host-Keys");
    expect(screen.getByTestId("data-path-wrappingFile")).toHaveTextContent(
      "Schlüsseldatei des Master-Passworts",
    );
    expect(screen.getByTestId("data-path-mcpSettings")).toHaveTextContent("MCP-Einstellungen");
    expect(screen.getByTestId("data-path-sessions")).toHaveTextContent(
      "in der Datenbank gespeichert",
    );
  });

  it("zeigt Einträge einer Edition mit deren Beschriftung", async () => {
    vi.mocked(getDataPaths).mockResolvedValue([
      ...ENTRIES,
      { id: "license", label: "Lizenzdatei", path: "/data/license.json", isDirectory: false },
    ]);

    renderSection();

    expect(await screen.findByTestId("data-path-license")).toHaveTextContent("Lizenzdatei");
    expect(screen.getByTestId("data-path-license")).toHaveTextContent("/data/license.json");
  });

  it("kopiert den Pfad in die Zwischenablage", async () => {
    vi.mocked(getDataPaths).mockResolvedValue(ENTRIES);
    writeText.mockResolvedValue(undefined);

    renderSection();
    fireEvent.click(await screen.findByLabelText("Kopieren: Host-Keys"));

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("/data/host_keys.json"));
    expect(await screen.findByText("Pfad kopiert")).toBeInTheDocument();
  });

  it("öffnet den Ordner über den Bezeichner, nie über einen Pfad", async () => {
    vi.mocked(getDataPaths).mockResolvedValue(ENTRIES);
    vi.mocked(openDataPathFolder).mockResolvedValue(undefined);

    renderSection();
    fireEvent.click(await screen.findByLabelText("Ordner öffnen: MCP-Einstellungen"));

    await waitFor(() => expect(openDataPathFolder).toHaveBeenCalledWith("mcpSettings"));
  });

  it("zeigt einen sichtbaren Fehler, wenn der Ordner nicht geöffnet werden kann", async () => {
    vi.mocked(getDataPaths).mockResolvedValue(ENTRIES);
    vi.mocked(openDataPathFolder).mockRejectedValue("folder does not exist");

    renderSection();
    fireEvent.click(await screen.findByLabelText("Ordner öffnen: Datenbank"));

    expect(
      await screen.findByText(
        "Ordner konnte nicht geöffnet werden: folder does not exist",
      ),
    ).toBeInTheDocument();
  });

  it("zeigt einen sichtbaren Fehler, wenn die Pfade nicht geladen werden können", async () => {
    vi.mocked(getDataPaths).mockRejectedValue(new Error("boom"));

    renderSection();

    expect(
      await screen.findByText("Datenpfade konnten nicht geladen werden."),
    ).toBeInTheDocument();
  });
});
