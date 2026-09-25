// Spec 0075, §3.1.7/§5.2a: Vorschau-Dialog vor dem `ssh_config`-Import.
// Deckt genau die interaktiven Stellen ab, die ein reiner Typ-Check nicht
// prüft: die Liste der Dateien, die auf Weg (b) geöffnet würden, wird bei
// jeder Änderung der Wahl neu berechnet (nicht die statische
// `identityFilePaths`-Liste aus dem DTO); ein buchstäbliches Schlagwort ist
// abwählbar und geht bei Abwahl als `droppedTags` in die Bestätigung; die
// Vorgabe folgt Q-BL-0216-02: ein buchstäbliches Schlagwort, das eine
// bestehende Tag-Allow-Regel trifft, ist standardmäßig abgewählt, jedes
// andere bleibt angewählt (s. `defaultTagSelected`).
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

// Q-BL-0216-02: derselbe Grundriss wie `preview()`, aber mit einem
// zusätzlichen buchstäblichen Schlagwort, das **nur** eine Deny-Regel
// trifft — als Gegenstück zum buchstäblichen "prod" (trifft eine
// Allow-Regel) in der gemeinsamen Fixture.
function previewWithDenyOnlyLiteralTag(): SshConfigImportPreviewDto {
  const dto = preview();
  dto.entries[0].tags.push({
    tag: "onlydeny",
    origin: { file: "/tmp/config", line: 6, block: "onlydeny *" },
    matchedRules: [{ ruleId: "rule-3", action: "deny" }],
    isLiteral: true,
  });
  return dto;
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

  // spec-reviewer-Fund, Runde 2: die Action einer getroffenen Regel
  // (Allow/Confirm/Deny) wurde nirgends per Test geprüft — ein Rückfall auf
  // nur die Anzahl wäre unbemerkt geblieben.
  it("shows which rule action matched a flagged tag (§5.2a)", async () => {
    vi.mocked(previewSshConfigImport).mockResolvedValue(preview());
    renderDialog();
    await screen.findByTestId("entry-0-name");

    // Das buchstäbliche Schlagwort "prod" trifft eine allow-Regel.
    const literalTagLabel = screen.getByText("prod").closest("label");
    expect(literalTagLabel?.textContent).toContain("Allow");
    // Das Muster-Schlagwort "*.prod.de" trifft eine confirm-Regel.
    const patternTagLabel = screen.getByText("*.prod.de").closest("label");
    expect(patternTagLabel?.textContent).toContain("Confirm");
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

  it("submits the default choice unchanged: identity mode keepAsFile, the literal Allow-tag pre-dropped (Q-BL-0216-02)", async () => {
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
      // "prod" ist buchstäblich und trifft eine Allow-Regel (rule-2) —
      // Q-BL-0216-02 lässt es standardmäßig abgewählt, ohne dass der Nutzer
      // etwas tut. "*.prod.de" (Muster, trifft nur eine Confirm-Regel)
      // bleibt angewählt.
      droppedTags: ["prod"],
    });
    expect(onImported).toHaveBeenCalled();
  });

  // Q-BL-0216-02 (Spec 0075, §9): Ein buchstäbliches Schlagwort, das eine
  // bestehende Tag-Allow-Regel trifft, ist in der Vorschau standardmäßig
  // abgewählt; trifft es nur eine Deny- oder Confirm-Regel (oder keine),
  // bleibt es angewählt. Gegen den alten Stand (`defaultTagSelected` gab
  // immer `true` zurück) rot gesehen: beide Erwartungen unten schlugen
  // fehl, weil "prod" (Allow-Treffer) als angewählt startete.
  it("Q-BL-0216-02: a literal tag hitting an Allow rule starts deselected, one hitting only Deny stays selected", async () => {
    vi.mocked(previewSshConfigImport).mockResolvedValue(previewWithDenyOnlyLiteralTag());
    renderDialog();
    await screen.findByText("web1");

    const allowHitCheckbox = screen.getByText("prod").closest("label")?.querySelector("input");
    const denyOnlyHitCheckbox = screen.getByText("onlydeny").closest("label")?.querySelector("input");
    expect(allowHitCheckbox).not.toBeChecked();
    expect(denyOnlyHitCheckbox).toBeChecked();
  });

  // spec-reviewer-Fund (Runde 1, I-1): `action` ist heute immer klein
  // geschrieben (`rule_action_key` in `ssh_config_apply.rs` ist exhaustiv),
  // aber ein Vergleich, der das voraussetzt, fällt bei einer künftigen
  // Änderung der DTO-Kodierung stillschweigend in die unsichere Richtung
  // (angewählt). Hält die für diesen Fall sichere Richtung fest.
  it("treats a differently-cased 'Allow' action as an allow match too", async () => {
    const dto = preview();
    // Die DTO-Kodierung sagt heute exhaustiv `"allow"` klein; der Cast
    // simuliert absichtlich eine andere Schreibweise, um `.toLowerCase()`
    // in `defaultTagSelected` zu verifizieren.
    (dto.entries[0].tags[1].matchedRules as unknown as Array<{ ruleId: string; action: string }>)[0].action =
      "Allow";
    vi.mocked(previewSshConfigImport).mockResolvedValue(dto);
    renderDialog();
    await screen.findByText("web1");

    const literalTagCheckbox = screen.getByText("prod").closest("label")?.querySelector("input");
    expect(literalTagCheckbox).not.toBeChecked();
  });

  it("reselecting the literal Allow-tag that starts deselected removes it from droppedTags on confirm", async () => {
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
    expect(literalTagCheckbox).not.toBeChecked();
    fireEvent.click(literalTagCheckbox as HTMLInputElement);

    fireEvent.click(screen.getByText("Importieren"));

    await waitFor(() => expect(applySshConfigImport).toHaveBeenCalled());
    const choices = vi.mocked(applySshConfigImport).mock.calls[0][0];
    expect(choices[0].droppedTags).toEqual([]);
  });

  // spec-reviewer-Fund (Runde 1, K-4): Der Test oben deckt nur noch die
  // "wieder anwählen"-Richtung von `toggleTag` (`dropped.delete`) ab, weil
  // "prod" bereits abgewählt startet. Ohne einen Test für die
  // "abwählen"-Richtung (`dropped.add`) fiele eine Regression, die ein von
  // Hand abgewähltes Schlagwort stillschweigend doch mit anlegt, niemandem
  // auf — genau die Fähigkeit, auf die §5.2a und §6.4.3a aufsetzen. Das
  // Muster-Schlagwort "*.prod.de" ist angewählt (nicht buchstäblich, s.
  // `defaultTagSelected`) und deckt zusätzlich ab, dass Nicht-Literale
  // wirklich mit der Vorgabe "angewählt" starten.
  it("deselecting the pattern tag by hand sends it as droppedTags on confirm", async () => {
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

    const patternTagCheckbox = screen.getByText("*.prod.de").closest("label")?.querySelector("input");
    expect(patternTagCheckbox).toBeTruthy();
    expect(patternTagCheckbox).toBeChecked();
    fireEvent.click(patternTagCheckbox as HTMLInputElement);
    expect(patternTagCheckbox).not.toBeChecked();

    fireEvent.click(screen.getByText("Importieren"));

    await waitFor(() => expect(applySshConfigImport).toHaveBeenCalled());
    const choices = vi.mocked(applySshConfigImport).mock.calls[0][0];
    // "prod" ist ohnehin schon per Vorgabe abgewählt; nach dem Klick auf
    // "*.prod.de" sind es beide.
    expect(choices[0].droppedTags.sort()).toEqual(["*.prod.de", "prod"]);
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
