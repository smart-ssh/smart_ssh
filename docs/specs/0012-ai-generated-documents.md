# Spec: KI-generierte Dokumente

Status: Entwurf
Modul: Erweiterung `core::ai` (Spec 0006), `crates/app-tauri` + `frontend/`
Abhängigkeiten: `AiAction` (Spec 0003), Chat-Kernschleife (Spec 0007,
Abschnitt 6)

## 1. Ziel

Der Nutzer kann die KI im Chat bitten, eine Analyse/Zusammenfassung als
formatiertes Dokument bereitzustellen (z. B. "gib mir ein Dokument mit der
Analyse"). Die KI liefert strukturierten Markdown-Inhalt, der Nutzer kann ihn
direkt als Markdown- oder Word-Datei speichern.

## 2. Neue Aktion

Ergänzung zu `AiAction` (Spec 0003, Abschnitt 5.2):

```rust
pub enum AiAction {
    SuggestCommand { command: String },
    ProposeNoteUpdate { target: NoteTarget, new_content: String },
    GenerateDocument { title: String, content_markdown: String },
}
```

`GenerateDocument` durchläuft **nicht** die Filter-Engine (Spec 0002) — es
betrifft weder SSH noch den Server, sondern erzeugt reinen lokalen Inhalt.
Es wird auch **nicht automatisch auf die Festplatte geschrieben** — Inhalte
landen erst bei explizitem Nutzer-Klick auf der Festplatte, konsistent mit
dem generellen Prinzip, dass Dateizugriffe eine bewusste Nutzeraktion
brauchen.

## 3. Ablauf

1. KI liefert `AiEvent::ActionProposed(AiAction::GenerateDocument { title, content_markdown })`.
2. Backend leitet das **direkt** (ohne Zwischenschritt, kein Bestätigungsdialog
   nötig — es passiert ja noch nichts Persistentes) als
   `chat-document-generated` Event ans Frontend weiter:
   ```
   chat-document-generated { session_id, action_id, title, content_markdown }
   ```
3. Frontend zeigt den Inhalt als eigene, hübsch gerenderte Karte im
   Chatverlauf (gerendertes Markdown, nicht Rohtext), mit zwei Buttons:
   **"Als Markdown speichern"** und **"Als Word speichern"**.
4. Klick auf einen der Buttons ruft
   `export_document(content_markdown: String, title: String, format: DocumentFormat) -> ()`
   auf. Das öffnet einen nativen Speichern-unter-Dialog (Tauri
   Dialog-Plugin), vorbelegt mit einem aus `title` abgeleiteten Dateinamen
   und der passenden Endung. Erst nach Bestätigung im nativen Dialog wird
   tatsächlich geschrieben.

```rust
pub enum DocumentFormat { Markdown, Word }
```

## 4. Word-Konvertierung

Für `DocumentFormat::Word` wird der Markdown-Inhalt in ein einfaches DOCX
umgewandelt (z. B. über die `docx-rs`-Crate). MVP-Scope der Konvertierung:
Überschriften (`#`–`###`), Absätze, Fett/Kursiv, Aufzählungen/nummerierte
Listen. Komplexere Markdown-Konstrukte (Tabellen, Code-Blöcke,
verschachtelte Listen) werden vereinfacht dargestellt (z. B. Code-Blöcke als
Absatz in Monospace-Schrift ohne Syntax-Highlighting) statt einen vollen
Markdown-zu-DOCX-Renderer nachzubauen — das ist für den Anwendungsfall
"Analyse-Dokument" ausreichend.

## 5. Kontext-Konsistenz

Der generierte Dokumentinhalt wird als Teil der Assistant-Nachricht in
`context.history` übernommen (wie ein normaler Chat-Text), damit die KI sich
in der Folgekonversation darauf beziehen kann ("ergänze im Dokument noch
Abschnitt X") — kein Sonderfall gegenüber normalem Chat-Text nötig.

## 6. Offene Punkte

- Soll `available_actions` (Spec 0006) für `GenerateDocument` immer verfügbar
  sein, oder nur, wenn der Nutzer explizit danach fragt (Erkennung z. B.
  über einen Slash-Command `/dokument` statt reiner Freitext-Erkennung durch
  die KI)? Aktuell: immer als verfügbares Tool angeboten, die KI entscheidet
  modellseitig, wann es passt — konsistent mit dem bestehenden
  Tool-Calling-Ansatz für `SuggestCommand`.
- PDF als drittes Exportformat wäre naheliegend, aber nicht Teil dieser
  Spec — ließe sich später als dritter Button ergänzen, sobald eine
  DOCX→PDF- oder Markdown→PDF-Route feststeht.
