// Spec 0075, §3.1.7/§5.2a: Vorschau-Dialog vor dem `ssh_config`-Import.
// Deckt genau die interaktiven Stellen ab, die ein reiner Typ-Check nicht
// prüft: die Liste der Dateien, die auf Weg (b) geöffnet würden, wird bei
// jeder Änderung der Wahl neu berechnet (nicht die statische
// `identityFilePaths`-Liste aus dem DTO); ein buchstäbliches Schlagwort ist
// abwählbar und geht bei Abwahl als `droppedTags` in die Bestätigung; die
// Vorgabe entspricht dem Kern (alles angewählt, s.
// `defaultTagSelected`-Kommentar zu Q-BL-0216-02).
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { applySshConfigImport, previewSshConfigImport } from "../api";
import { testI18n } from "../testI18n";
import type { SshConfigImportPreviewDto } from "../types";
import { SshConfigImportDialog } from "./SshConfigImportDialog";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  previewSshConfigImport: vi.fn(),
  applySshConfigImport: vi.fn(),
}));

// Ohne dieses Zurücksetzen liest ein späterer Test `mock.calls[0]` eines
// früheren Tests mit — genau der Fehler, den Spec-Reviewer-Funde in anderen
// Testdateien dieses Repos schon einmal gefunden haben (verifiziert: ohne
// `beforeEach` unten meldet "deselecting the literal tag..." die
// `droppedTags` des vorherigen Tests, nicht die eigenen).
beforeEach(() => {
  vi.clearAllMocks();
});

function preview(): SshConfigImportPreviewDto {
  return {
    groups: [{ name: "config", parent: null, sourcePath: "/tmp/config" }],
    files: [{ path: "/tmp/config", depth: 0, status: "read" }],
    entries: [
      {
        index: 0,
        name: "web1",
        group: 0,
        host: { value: "10.0.0.1", origin: { file: "/tmp/config", line: 2, block: "web1" } },
        port: { value: 22, origin: null },
        username: { value: "", origin: null },
        tags: [
          {
            tag: "*.prod.de",
            origin: { file: "/tmp/config", line: 1, block: "*.prod.de" },
            matchedRules: [{ ruleId: "rule-1", action: "confirm" }],
            isLiteral: false,
          },
          {
            tag: "prod",
            origin: { file: "/tmp/config", line: 5, block: "prod *" },
            matchedRules: [{ ruleId: "rule-2", action: "allow" }],
            isLiteral: true,
          },
        ],
        identityFile: {
          path: "/keys/id_ed25519",
          origin: { file: "/tmp/config", line: 3, block: "web1" },
          usableWhenConnecting: true,
        },
        jumpHost: { name: "bastion", existing: false },
        conflict: null,
      },
      {
        index: 1,
        name: "bastion",
        group: 0,
        host: { value: "10.0.0.9", origin: null },
        port: { value: 22, origin: null },
        username: { value: "", origin: null },
        tags: [],
        identityFile: null,
        jumpHost: null,
        conflict: null,
      },
    ],
    skipped: [{ file: "/tmp/config", line: 9, directive: "Compression", reason: "unsupported", entry: null }],
    identityFilePaths: ["/keys/id_ed25519"],
  };
}

function renderDialog(onImported = vi.fn(), onClose = vi.fn()) {
  render(
    <I18nextProvider i18n={testI18n}>
      <SshConfigImportDialog onClose={onClose} onImported={onImported} />
    </I18nextProvider>,
  );
  return { onImported, onClose };
}

describe("SshConfigImportDialog (Spec 0075, §3.1.7)", () => {
  it("shows the preview: entries, origin of a value from the file, and the literal-tag marker", async () => {
    vi.mocked(previewSshConfigImport).mockResolvedValue(preview());
    renderDialog();

    expect(await screen.findByTestId("entry-0-name")).toHaveTextContent("web1");
    expect(screen.getByTestId("entry-1-name")).toHaveTextContent("bastion");
    // Herkunft eines Werts aus der Datei (§3.1.3).
    expect(screen.getByText(/config:2/)).toBeInTheDocument();
    // Das buchstäbliche Schlagwort ist deutlicher markiert als das
    // Muster-Schlagwort (§5.2a) — hier: das Warnsymbol.
    const literalTagLabel = screen.getByText("prod").closest("label");
    expect(literalTagLabel?.textContent).toContain("⚠");
    const patternTagLabel = screen.getByText("*.prod.de").closest("label");
    expect(patternTagLabel?.textContent).not.toContain("⚠");
  });

  it("recomputes the 'will open' file list when the identity mode changes — not a static list", async () => {
    vi.mocked(previewSshConfigImport).mockResolvedValue(preview());
    renderDialog();
    await screen.findByText("web1");

    // Vorgabe (a): nichts wird geöffnet — die Warnbox mit der "würde
    // gelesen"-Liste erscheint nicht (der Pfad selbst steht ohnehin am
    // Eintrag, das prüft die Warnbox nicht mit).
    expect(screen.queryByText(/würden beim Bestätigen gelesen/)).not.toBeInTheDocument();

    // Global auf Weg (b) umgestellt (nicht den per-Eintrag-Radio mit
    // demselben Label — beide existieren gleichzeitig, §3.1.9 "für den
    // ganzen Import und einzeln je Eintrag"): jetzt erscheint die Warnbox
    // mit dem Pfad, *bevor* irgendetwas bestätigt wurde.
    const globalSection = screen.getByText("Schlüsseldatei (IdentityFile) — Vorgabe für alle Einträge").closest("section");
    fireEvent.click(within(globalSection as HTMLElement).getByRole("radio", { name: "Einlesen und im Schlüsselbund ablegen" }));
    const warnBox = await screen.findByText(/würden beim Bestätigen gelesen/);
    expect(warnBox.closest("div")).toHaveTextContent("/keys/id_ed25519");
  });

  it("submits the default choice unchanged: identity mode keepAsFile, no dropped tags", async () => {
    vi.mocked(previewSshConfigImport).mockResolvedValue(preview());
    vi.mocked(applySshConfigImport).mockResolvedValue({
      createdServers: 2,
      createdGroups: 1,
      skippedConflicts: 0,
      identityFallbacks: [],
      identityEncrypted: [],
    });
    const { onImported } = renderDialog();
    await screen.findByText("web1");

    fireEvent.click(screen.getByText("Importieren"));

    await waitFor(() => expect(applySshConfigImport).toHaveBeenCalled());
    const choices = vi.mocked(applySshConfigImport).mock.calls[0][0];
    expect(choices).toHaveLength(2);
    expect(choices[0]).toMatchObject({
      index: 0,
      selected: true,
      identityMode: "keepAsFile",
      droppedTags: [],
    });
    expect(onImported).toHaveBeenCalled();
  });

  it("deselecting the literal tag sends it as droppedTags on confirm", async () => {
    vi.mocked(previewSshConfigImport).mockResolvedValue(preview());
    vi.mocked(applySshConfigImport).mockResolvedValue({
      createdServers: 2,
      createdGroups: 1,
      skippedConflicts: 0,
      identityFallbacks: [],
      identityEncrypted: [],
    });
    renderDialog();
    await screen.findByText("web1");

    const literalTagCheckbox = screen.getByText("prod").closest("label")?.querySelector("input");
    expect(literalTagCheckbox).toBeTruthy();
    fireEvent.click(literalTagCheckbox as HTMLInputElement);

    fireEvent.click(screen.getByText("Importieren"));

    await waitFor(() => expect(applySshConfigImport).toHaveBeenCalled());
    const choices = vi.mocked(applySshConfigImport).mock.calls[0][0];
    expect(choices[0].droppedTags).toEqual(["prod"]);
  });

  it("cancel closes the dialog without applying anything", async () => {
    vi.mocked(previewSshConfigImport).mockResolvedValue(preview());
    const { onClose } = renderDialog();
    await screen.findByText("web1");

    fireEvent.click(screen.getByText("Abbrechen"));

    expect(onClose).toHaveBeenCalled();
    expect(applySshConfigImport).not.toHaveBeenCalled();
  });

  it("closes immediately (no dialog) when the user cancels the native file picker (preview resolves null)", async () => {
    vi.mocked(previewSshConfigImport).mockResolvedValue(null);
    const { onClose } = renderDialog();

    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });
});
