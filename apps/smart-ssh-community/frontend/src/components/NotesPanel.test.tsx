// Spec 0030, Abschnitt 5: Historie mit mindestens drei Revisionen — die
// mittlere zeigt beim Aufklappen korrekt den Diff gegenüber der direkt
// vorherigen (nicht gegenüber der ältesten oder der aktuellen), die
// älteste zeigt "Ursprüngliche Version" ohne Diff-Darstellung.
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { NoteRevisionDto } from "../types";
import { NotesPanel } from "./NotesPanel";

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

vi.mock("../api", () => ({
  listNoteRevisions: vi.fn(() => Promise.resolve(revisions)),
  rollbackNote: vi.fn(() => Promise.resolve()),
  updateServerNotes: vi.fn(() => Promise.resolve()),
  updateGroupNotes: vi.fn(() => Promise.resolve()),
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
