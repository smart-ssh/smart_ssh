# 0018-ai-generated-document-export-design

## Status
Vorgeschlagen

## Kontext

`docs/specs/0012-ai-generated-documents.md` nennt für die Word-Konvertierung
nur beispielhaft "z. B. über die `docx-rs`-Crate" und skizziert keine
konkrete API — weder für die Markdown-Konvertierung selbst noch dafür, wie
`AiAction::GenerateDocument` in die bestehende `run_one_round()`-Ereignis-
schleife (Spec 0006/0007) eingehängt wird. Drei Punkte mussten deshalb ohne
explizite Spec-Vorgabe entschieden werden.

## Entscheidungen

**1. `docx-rs` (crates.io, `bokuweb/docx-rs`, Version 0.4.22) — bestätigt,
aber mit einer API, die von der Spec-Skizze abweicht.** Es gibt keinen
einzelnen `save(path)`-Aufruf. Ein Dokument wird als Builder-Kette aus
`Docx::new()...add_paragraph(...)` aufgebaut, per `.build()` in ein
`XMLDocx`-Zwischenformat überführt und dann via `.pack(writer)` gegen ein
`Write + Seek`-Ziel geschrieben — in `markdown_to_docx_bytes()`
(`crates/app-tauri/src/document_export.rs`) ein `Cursor<Vec<u8>>`, damit die
Konvertierung selbst IO-frei und ohne Tauri-Laufzeit testbar bleibt (das
eigentliche Schreiben auf die Festplatte passiert erst im
`export_document`-Command, nach Bestätigung im nativen Speichern-Dialog).
Wichtiger: Es gibt **keine impliziten "eingebauten" Word-Stile** — ein
Absatz mit `.style("Heading1")` sieht in Word nur dann wie eine Überschrift
aus, wenn der Stil `"Heading1"` zuvor selbst mit Formatierung (fett, Größe)
über `Docx::add_style(Style::new(...).bold().size(...))` registriert wurde.
Ebenso ist eine Liste kein High-Level-"Bullet-List"-Konzept, sondern erfordert
eine explizit registrierte `AbstractNumbering`/`Numbering`-Definition, auf
die einzelne Absätze sich per `NumberingId`/`IndentLevel` beziehen.

**2. `pulldown-cmark` für das Markdown-Parsing — von der Spec nicht
genannt, aber notwendig.** Statt eines handgeschriebenen Zeilen-Parsers
läuft die Konvertierung einmal linear über `pulldown-cmark`s
Event-Stream (`Start(Tag)`/`End(TagEnd)`/`Text`/…) und baut dabei
paragraphenweise die docx-rs-Struktur auf. Bewusst **ohne**
`Options::ENABLE_TABLES`: dadurch behandelt `pulldown-cmark` eine
Markdown-Tabelle automatisch als reinen Absatztext (Zeilen mit
`|`-Zeichen) — das ergibt exakt die von Spec-Abschnitt 4 verlangte
vereinfachte Tabellendarstellung, ganz ohne eigenen Tabellen-Renderer.
Verschachtelte Listen bekommen bis zu drei unterscheidbare
Einrückungsstufen (danach flachen tiefere Ebenen auf die letzte ab) —
ebenfalls eine bewusste Vereinfachung im vom Spec-Text explizit erlaubten
Rahmen ("Komplexere Markdown-Konstrukte … werden vereinfacht dargestellt").

**3. `GenerateDocument` wird in `run_one_round()` per eigenem, spezifischerem
Match-Arm abgefangen, bevor der generische `AiEvent::ActionProposed`-Zweig
greift, und zählt **nicht** als `executed_action` für den automatischen
Folgerunden-Mechanismus (ADR 0014).** Spec 0012 Abschnitt 2 verlangt nur
"kein Filter-Engine-Aufruf, kein Bestätigungsdialog", sagt aber nichts zum
Folgerunden-Verhalten. Ein `SuggestCommand`/`ProposeNoteUpdate` löst nach
Ausführung eine automatische KI-Folgerunde aus, damit die KI das
Kommando-Ergebnis interpretieren kann — ein bereits geliefertes Dokument
ist dagegen schon eine vollständige Antwort, die keine weitere Interpretation
braucht. Das wird zusätzlich durch einen Test abgesichert
(`test_generate_document_emits_event_without_filter_engine_or_confirmation`),
der bewusst *ohne* Confirm-Responder läuft, um zu beweisen, dass der
Confirm-Wartepfad für diese Aktion nie erreicht wird.

## Konsequenzen

**Positiv:**
- Die Markdown→DOCX-Konvertierung ist eine reine, synchrone Funktion ohne
  Tauri-/Dateisystem-Abhängigkeit — vollständig durch Unit-Tests abgedeckt,
  inklusive eines Roundtrip-Tests, der das erzeugte DOCX über `docx_rs::
  read_docx()` zurückliest und auf erwartete Textfragmente prüft (statt sich
  nur auf "keine Panik" zu verlassen).
- Tabellen- und Verschachtelungs-Vereinfachung fallen ohne zusätzlichen Code
  aus der `pulldown-cmark`-Konfiguration heraus, statt eigens implementiert
  werden zu müssen.
- Der neue `GenerateDocument`-Pfad greift nicht in die drei bestehenden
  exhaustiven `AiAction`-Matches (`evaluate_action`, `handle_user_decision`,
  `execute_action`) ein — dort markiert je ein `unreachable!()` das
  Invariante explizit, statt die Funktionen mit einem Sonderfall zu
  belasten, den sie nie zu Gesicht bekommen.

**Negativ / Trade-off:**
- Zwei zusätzliche Abhängigkeiten (`docx-rs`, `pulldown-cmark`) allein für
  dieses eine Feature — beide sind aber reine, gut isolierte
  Konvertierungs-Bibliotheken ohne Laufzeit-Fußabdruck außerhalb von
  `export_document`/`markdown_to_docx_bytes`.
- Die manuell registrierten Heading-/Listen-Stile sind eine eigene,
  hartkodierte Formatierung (Schriftgröße, Fett) statt Words tatsächlicher
  eingebauter Stilvorlagen — sieht in Word "wie eine Überschrift aus", ist
  aber technisch kein offizieller `Heading1`-Stil, den z. B. eine
  automatische Inhaltsverzeichnis-Funktion erkennen würde. Für den
  Anwendungsfall "Analyse-Dokument exportieren" ohne ein solches Bedürfnis
  ausreichend.
- `std::fs::write` (statt `tokio::fs::write`) im `export_document`-Command
  blockiert kurzzeitig den Async-Runtime-Thread — bewusst in Kauf genommen,
  um nicht extra das `fs`-Feature von `tokio` nur für diesen einen, durch
  eine explizite Nutzeraktion ausgelösten Schreibvorgang zu aktivieren.
