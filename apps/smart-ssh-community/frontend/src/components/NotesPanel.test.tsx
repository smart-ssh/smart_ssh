// Spec 0030, Abschnitt 5: Historie mit mindestens drei Revisionen — die
// mittlere zeigt beim Aufklappen korrekt den Diff gegenüber der direkt
// vorherigen (nicht gegenüber der ältesten oder der aktuellen), die
// älteste zeigt "Ursprüngliche Version" ohne Diff-Darstellung.
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { NoteRevisionDto } from "../types";
import { NotesPanel } from "./NotesPanel";

// spec-reviewer-Fund (Spec 0058, Review des Politur-Pakets): jsdom kennt
// `scrollIntoView` nicht — die autoFocus-Tests unten überschreiben es global
// auf `Element.prototype`. Ohne Wiederherstellung würde dieser Stub in JEDEN
// später in derselben Datei laufenden Test durchsickern.
const originalScrollIntoView = Element.prototype.scrollIntoView;
afterEach(() => {
  Element.prototype.scrollIntoView = originalScrollIntoView;
});

const revisions: NoteRevisionDto[] = [
  {
    id: "rev-1-oldest",
    content: "Zeile A\nZeile B",
    editedBy: { kind: "user" },
    createdAt: "2026-01-01T10:00:00Z",
  },
  {
    id: "rev-2-middle",
    content: "Zeile A\nZeile B geändert",
    editedBy: { kind: "user" },
    createdAt: "2026-01-02T10:00:00Z",
  },
  {
    id: "rev-3-current",
    content: "Zeile A\nZeile B geändert\nZeile C neu",
    editedBy: { kind: "ai", provider: "anthropic", model: "claude" },
    createdAt: "2026-01-03T10:00:00Z",
  },
];

// Spec 0058, Teil 1: ein hoher Default-Schwellwert, damit die bestehenden
// Historie-Tests (kurze Beispielnotizen) den neuen Hinweis nicht versehentlich
// mitrendern — die eigenen Hinweis-Tests unten setzen ihn gezielt niedriger.
// `mock`-Namenspräfix erforderlich: Vitest hoisted `vi.mock`-Factories über
// alle anderen Top-Level-Deklarationen hinweg, referenzierte Variablen
// dürfen deshalb nur mit diesem Präfix vorab initialisiert sein.
const mockLargeNoteDialogThresholdBytes = vi.fn(() => Promise.resolve(1_000_000));
const mockRequestNoteShrink = vi.fn((_serverId: string) => Promise.resolve());

vi.mock("../api", () => ({
  listNoteRevisions: vi.fn(() => Promise.resolve(revisions)),
  rollbackNote: vi.fn(() => Promise.resolve()),
  updateServerNotes: vi.fn(() => Promise.resolve()),
  updateGroupNotes: vi.fn(() => Promise.resolve()),
  largeNoteDialogThresholdBytes: () => mockLargeNoteDialogThresholdBytes(),
  requestNoteShrink: (serverId: string) => mockRequestNoteShrink(serverId),
  commandErrorMessage: (err: unknown) => String(err),
}));

async function renderWithHistoryOpen() {
  render(
    <I18nextProvider i18n={testI18n}>
      <NotesPanel
        // Bewusst disjunkt von jeglichem Revisionsinhalt ("Zeile A/B/C") —
        // ein `<textarea>`-Wert zählt in jsdom als Text-Inhalt und würde
        // sonst `getByText`-Abfragen unten mehrdeutig machen.
        target={{ Server: "server-1" }}
        currentNotes="Aktueller Entwurf, nicht Teil der Historie"
        onNotesChanged={() => {}}
      />
    </I18nextProvider>,
  );
  fireEvent.click(screen.getByText("Historie anzeigen"));
  // Warten, bis `listNoteRevisions()` aufgelöst und die Liste gerendert ist.
  await screen.findAllByText("Wiederherstellen");
}

describe("note history diff (Spec 0030)", () => {
  it("collapses every entry by default", async () => {
    await renderWithHistoryOpen();
    // Zeitpunkt/Editor/Wiederherstellen bleiben sichtbar ...
    expect(screen.getAllByText("Wiederherstellen")).toHaveLength(3);
    // ... aber kein Inhalt/Diff, solange nichts aufgeklappt wurde.
    expect(screen.queryByText("Ursprüngliche Version")).toBeNull();
    expect(screen.queryByText(/Zeile C neu/)).toBeNull();
  });

  it("shows the middle revision's diff against its direct predecessor, not the oldest or current", async () => {
    await renderWithHistoryOpen();

    // Drei Zeitpunkt/Editor-Zeilen in Dokumentreihenfolge = chronologisch
    // aufsteigend (ältesteste zuerst, s. Backend-`ORDER BY created_at`) —
    // Index 1 ist damit die mittlere Revision, unabhängig vom
    // Datumsformat der Laufzeit-Locale.
    const toggles = screen.getAllByRole("button", { name: /Nutzer|KI \(/ });
    expect(toggles).toHaveLength(3);
    fireEvent.click(toggles[1]);

    // Diff rev-2 (Zeile A\nZeile B geändert) gegen rev-1 (Zeile A\nZeile B):
    // nur "Zeile B" entfernt und "Zeile B geändert" hinzugefügt — nicht der
    // Sprung gegen rev-3 (der zusätzlich "Zeile C neu" enthielte).
    expect(screen.getByText("Zeile B geändert")).toBeInTheDocument();
    expect(screen.getByText("Zeile B")).toBeInTheDocument();
    expect(screen.queryByText("Zeile C neu")).toBeNull();
    expect(screen.queryByText("Ursprüngliche Version")).toBeNull();
  });

  it("shows the oldest revision as 'Ursprüngliche Version' with full content, no diff", async () => {
    const { container } = render(
      <I18nextProvider i18n={testI18n}>
        <NotesPanel
          target={{ Server: "server-1" }}
          currentNotes="Aktueller Entwurf, nicht Teil der Historie"
          onNotesChanged={() => {}}
        />
      </I18nextProvider>,
    );
    fireEvent.click(screen.getByText("Historie anzeigen"));
    await screen.findAllByText("Wiederherstellen");

    const toggles = screen.getAllByRole("button", { name: /Nutzer|KI \(/ });
    fireEvent.click(toggles[0]);

    const heading = screen.getByText("Ursprüngliche Version");
    // Voller Inhalt direkt unter der Beschriftung, nicht nur geänderte
    // Zeilen wie bei einem Diff.
    expect(heading.nextElementSibling?.textContent).toBe(revisions[0].content);
    // Die Diff-Komponente (erkennbar an ihrer `font-mono`-Klasse) wird für
    // die älteste Revision gar nicht gerendert.
    expect(container.querySelector(".font-mono")).toBeNull();
  });
});

// Spec 0058, Teil 2 (Etappe-4-Review-Fund): "Mache ich selbst" öffnete
// bisher nur das Formular, ohne zum Notizfeld zu scrollen/es zu
// fokussieren — der Nutzer musste es erst suchen.
describe("autoFocus (Spec 0058, Teil 2)", () => {
  it("scrolls to and focuses the textarea when autoFocus is true", () => {
    const scrollIntoView = vi.fn();
    // jsdom implementiert `scrollIntoView` nicht — ohne diesen Stub würfe
    // der Aufruf in `NotesPanel`s Mount-Effekt.
    Element.prototype.scrollIntoView = scrollIntoView;

    render(
      <I18nextProvider i18n={testI18n}>
        <NotesPanel
          target={{ Server: "server-1" }}
          currentNotes="Eine Notiz"
          onNotesChanged={() => {}}
          autoFocus
        />
      </I18nextProvider>,
    );

    const textarea = screen.getByRole("textbox");
    expect(scrollIntoView).toHaveBeenCalled();
    expect(textarea).toHaveFocus();
  });

  it("does not scroll or focus when autoFocus is false (the regular navigation case)", () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;

    render(
      <I18nextProvider i18n={testI18n}>
        <NotesPanel
          target={{ Server: "server-1" }}
          currentNotes="Eine Notiz"
          onNotesChanged={() => {}}
        />
      </I18nextProvider>,
    );

    const textarea = screen.getByRole("textbox");
    expect(scrollIntoView).not.toHaveBeenCalled();
    expect(textarea).not.toHaveFocus();
  });
});

// Spec 0058, Teil 1 (Session-Modell Etappe 5): proaktiver Hinweis beim
// Bearbeiten einer großen Notiz — das Gegenstück zum Sitzungsende-Dialog
// (Etappe 4), derselbe Schwellwert (hier über `largeNoteDialogThresholdBytes`
// gemockt, in der echten App vom Backend geliefert).
describe("large note hint (Spec 0058, Teil 1)", () => {
  it("shows the hint and a working 'jetzt zusammenfassen' link when the note is over the threshold", async () => {
    mockLargeNoteDialogThresholdBytes.mockResolvedValueOnce(10);

    render(
      <I18nextProvider i18n={testI18n}>
        <NotesPanel
          target={{ Server: "server-1" }}
          currentNotes="Eine Notiz, die den Schwellwert von 10 Byte klar überschreitet."
          onNotesChanged={() => {}}
        />
      </I18nextProvider>,
    );

    await screen.findByText(/sehr groß und kann bei langen Sitzungen/);
    expect(
      screen.getByText(/Die gespeicherte Notiz bleibt vollständig erhalten/),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByText("Jetzt zusammenfassen"));
    expect(mockRequestNoteShrink).toHaveBeenCalledWith("server-1");
  });

  it("counts UTF-8 bytes, not UTF-16 code units (multi-byte characters)", async () => {
    // "ä" ist 1 UTF-16-Code-Einheit (`string.length`), aber 2 UTF-8-Byte —
    // ein Rückfall von `utf8ByteLength` auf `draft.length` würde diesen Test
    // nicht bestehen (10 Zeichen, `.length` = 10 < 15, aber 20 UTF-8-Byte
    // >= 15).
    mockLargeNoteDialogThresholdBytes.mockResolvedValueOnce(15);
    const note = "ä".repeat(10);
    expect(note.length).toBeLessThan(15);
    expect(new TextEncoder().encode(note).length).toBeGreaterThanOrEqual(15);

    render(
      <I18nextProvider i18n={testI18n}>
        <NotesPanel target={{ Server: "server-1" }} currentNotes={note} onNotesChanged={() => {}} />
      </I18nextProvider>,
    );

    await screen.findByText(/sehr groß und kann bei langen Sitzungen/);
  });

  it("does not show the hint for a note below the threshold", async () => {
    mockLargeNoteDialogThresholdBytes.mockResolvedValueOnce(1_000_000);

    render(
      <I18nextProvider i18n={testI18n}>
        <NotesPanel target={{ Server: "server-1" }} currentNotes="Kurze Notiz." onNotesChanged={() => {}} />
      </I18nextProvider>,
    );

    // Auf den geladenen Schwellwert warten (sonst könnte der Test grün
    // durchlaufen, bevor `largeNoteDialogThresholdBytes()` überhaupt
    // aufgelöst hat, und nichts wirklich beweisen).
    await vi.waitFor(() => expect(mockLargeNoteDialogThresholdBytes).toHaveBeenCalled());

    expect(screen.queryByText(/sehr groß und kann bei langen Sitzungen/)).toBeNull();
  });

  it("does not show the 'jetzt zusammenfassen' link for a group note (no server to summarize)", async () => {
    mockLargeNoteDialogThresholdBytes.mockResolvedValueOnce(10);

    render(
      <I18nextProvider i18n={testI18n}>
        <NotesPanel
          target={{ Group: "group-1" }}
          currentNotes="Eine Gruppen-Notiz, die den Schwellwert von 10 Byte klar überschreitet."
          onNotesChanged={() => {}}
        />
      </I18nextProvider>,
    );

    await screen.findByText(/sehr groß und kann bei langen Sitzungen/);
    expect(screen.queryByText("Jetzt zusammenfassen")).toBeNull();
  });

  it("reacts live as the draft grows past the threshold while typing", async () => {
    mockLargeNoteDialogThresholdBytes.mockResolvedValueOnce(10);

    render(
      <I18nextProvider i18n={testI18n}>
        <NotesPanel target={{ Server: "server-1" }} currentNotes="Kurz" onNotesChanged={() => {}} />
      </I18nextProvider>,
    );

    await vi.waitFor(() => expect(mockLargeNoteDialogThresholdBytes).toHaveBeenCalled());
    expect(screen.queryByText(/sehr groß und kann bei langen Sitzungen/)).toBeNull();

    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "Eine deutlich längere Notiz, die jetzt über den Schwellwert wächst." },
    });

    expect(screen.getByText(/sehr groß und kann bei langen Sitzungen/)).toBeInTheDocument();
  });
});
