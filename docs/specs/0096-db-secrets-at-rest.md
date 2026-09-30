# Spec 0096 — Geheimnisse in der Datenbank: Sitzungstitel schwärzen, Rohdatei-Nachweis

Status: freigegeben · Backlog: BL-0295 · Gate: release-1.0/C
Zweck: Ein Kommando wie `mysql -p'geheim'` hinterlässt `geheim` nicht im Klartext in der Datenbankdatei — belegt durch einen Test, der die Datei selbst durchsucht.
Review-Priorität: ERHÖHT (Redaction, Persistenz)

## Getroffene Entscheidungen

- **E1 — Verschlüsselung ist der Schutz für den Verlauf.** Chatverlauf,
  Ausführungsprotokoll, Eingabe-Historie und Zusammenfassungen bleiben, wie
  sie sind (verschlüsselt, Inhalt unverändert). Der Verlauf zeigt weiter,
  was der Nutzer getippt hat.
- **E2 — Klartext-Stellen mit abgeleitetem Inhalt werden geschwärzt:** der
  von der KI erzeugte Sitzungstitel.
- **E3 — Notizen bleiben Klartext.** Was der Nutzer selbst schreibt,
  bleibt unverändert. **Notizvorschläge der KI werden geschwärzt, bevor der
  Nutzer sie im Vergleichsdialog sieht** — wie heute schon beim Kürzen.
  Was er bestätigt, ist genau das, was gespeichert wird.

## 1. Ist-Stand (origin/main aa316fd)

1. **Verschlüsselt** (ChaCha20-Poly1305, Nonce je Aufruf, Schlüssel
   `app:chat_content_encryption_key` im OS-Schlüsselbund,
   `crypto::chacha`, `crypto::key`): `chat_messages.content`,
   `ledger_entries.content`, `prompt_history.content`,
   `chat_sessions.summary_text`. Ohne Schlüssel sind diese Stores `None`,
   es wird nichts gespeichert (`app_shell` Start). Alt-Klartextzeilen der
   Eingabe-Historie werden beim Start verschlüsselt (gelesen).
2. **Klartext** (Migrationen gelesen): `chat_sessions.title`,
   `note_revisions.content`, `servers.notes`, `groups.notes`,
   `filter_rules.pattern_value`, `ai_provider_configs.*`
   (`extra_headers` → BL-0297).
3. **KI-Notizvorschläge:** Ein bestätigter `ProposeNoteUpdate` wird ohne
   Redactor als `NoteEditor::Ai` in `note_revisions.content` und die Notiz
   geschrieben (`persist_note_revision` in `orchestration/notes.rs`). Der
   Kürzungspfad derselben Datei schwärzt sein KI-Ergebnis dagegen schon
   (`redactor.redact_text(&text)` vor dem Speichern).
4. **Regel-Schnellvorschlag „Exakt":** legt eine Filterregel an, deren
   Muster wörtlich das vorgeschlagene Kommando ist
   (`rule_suggestions.rs`, `pattern_value: trimmed`), Klartext in
   `filter_rules.pattern_value`.
5. **Sitzungstitel:** `generate_session_title_on_disconnect` lässt die KI
   aus der per `reapply_redaction_for_send` redigierten History einen Titel
   erzeugen und speichert ihn nach `sanitize_generated_title` unverändert
   über `SqliteChatSessionStore::set_title_if_absent`. Die KI kann dabei
   Inhalt übernehmen, den der Redactor nicht erkannt hat, oder ihn aus dem
   Zusammenhang rekonstruieren. `sanitize_generated_title` kürzt auf 60
   Zeichen. Weitere Schreibpfade: nur `rename_session` (manuell,
   Nutzertext); `create_session` schreibt `NULL`.
6. **Tests:** Je Store prüft ein Test per `SELECT`, dass der BLOB den
   Klartext nicht enthält (`ledger_store`, `prompt_history_store`,
   `chat_session_store`). Kein Test durchsucht die Datenbankdatei selbst;
   SQLite kann Seiten zusätzlich in `-wal`/`-journal`-Dateien halten.
   `persistence-sqlite` hat `tempfile` als dev-dependency;
   `SqliteProfileStore::connect(path)` öffnet eine echte Datei; WAL ist per
   Migration `0001` gesetzt; `pool` ist `pub(crate)`. Der Cipher ist als
   Trait injizierbar.

## 2. Teil 0

Teil 0: entfällt. Alles ist mit Tests gegen eine Datei in einem
Temp-Verzeichnis prüfbar.

## 3. Ziel und Nicht-Ziele

Ziel: Nach einer Sitzung, in der ein Geheimnis in Prompt, Kommando,
Ausgabe, KI-Antwort und Zusammenfassung vorkam, enthält keine Datei des
Datenbank-Verzeichnisses das Geheimnis im Klartext — außer in Notizen, die
der Nutzer selbst schreibt, und in Filterregeln aus dem Schnellvorschlag
„Exakt" (§3 Nicht-Ziele).

Nicht-Ziele:
- Schwärzen **im** verschlüsselten Verlauf (Option B der Vorlage).
- Notizen schwärzen oder verschlüsseln (E3).
- Manuell vergebene Sitzungstitel.
- Filterregeln aus dem Schnellvorschlag „Exakt": Die Regel braucht das
  Kommando wörtlich und entsteht durch eine bewusste Nutzerhandlung.
- Klartext-Reste alter Eingabe-Historie in freien Seiten einer
  Bestands-Datenbank (Umstellung auf Verschlüsselung per `UPDATE`, ohne
  `VACUUM`/`secure_delete`) — nicht gemessen; nur Datenbanken aus der Zeit
  vor der Verschlüsselung.
- `ai_provider_configs.extra_headers` (BL-0297).
- Neue Redactor-Muster (Spec 0095).

## 4. Anforderungen

**A1 — Titel geschwärzt.** MUSS: Ein von der KI erzeugter Sitzungstitel
läuft vor dem Speichern durch den Session-Redactor, und zwar **vor** dem
Kürzen auf 60 Zeichen. Enthält er danach nur
noch Platzhalter und Leerraum, wird kein Titel gespeichert (wie heute bei
leerem Text).

**A2 — KI-Notizvorschläge geschwärzt.** MUSS: Der Inhalt jedes
Notizvorschlags der KI (`ProposeNoteUpdate`) läuft durch den
Session-Redactor, **bevor** er dem Nutzer angezeigt wird. Erfasst sind
die Wege, auf denen eine KI den Inhalt erzeugt: KI im Chat, beim Trennen
der Sitzung und ein externer Agent über MCP. **Nicht** erfasst ist
„In Notiz übernehmen": Dort wählt der Nutzer selbst eine Chatzeile aus
(E3). Da Chat-KI und „In Notiz übernehmen" denselben Einstieg mit
derselben Herkunft nutzen, geschieht die Schwärzung an den Einstiegen der
drei KI-Wege, nicht im gemeinsamen Pfad. Anzeige und Speichern nutzen
dieselbe geschwärzte Fassung.
Angezeigt, bestätigt und gespeichert wird die geschwärzte Fassung. Ist sie
leer, entsteht kein Vorschlag. Folge, bewusst hingenommen: Enthält die
bestehende Notiz ein vom Nutzer selbst geschriebenes Muster und übernimmt
die KI es in ihren Vorschlag, zeigt der Vergleichsdialog es als
`[REDACTED]`; der Nutzer sieht das vor dem Bestätigen.

**A3 — Rohdatei-Nachweis.** MUSS: Ein Test legt eine Datenbank in einem
Temp-Verzeichnis an, schreibt über die echten Stores mit Verschlüsselung
je einen Eintrag mit einem Geheimnis in: Chat-Nachricht (Text),
Kommando-Ergebnis (Kommando und Ausgabe), Ausführungsprotokoll,
Eingabe-Historie, Zusammenfassung. Danach schließt er die Verbindung und
durchsucht **jede Datei** des Verzeichnisses (Datenbank, `-wal`, `-shm`,
`-journal`) byteweise nach dem Geheimnis → kein Treffer.

**A4 — Der Nachweis kann scheitern.** MUSS: Derselbe Ablauf wie A3 läuft
ein zweites Mal gegen eine eigene Temp-Datenbank mit einem Cipher, der
Klartext durchreicht; dort muss dieselbe Suchfunktion das Geheimnis
**finden**. Findet sie es nicht, ist der Test rot.

## 5. Design

A3/A4 liegen in `persistence-sqlite` (dort ist der Pool schließbar);
gelesen wird erst nach `pool.close().await`. Für A1 genügt der vorhandene
Redactor der Session; kein neuer Store-Parameter.

## 6. Sicherheits-Invarianten

- **Redaction vor Datensenke:** verschärft (Titel).
- **Verschlüsselung:** unverändert; kein Pfad schreibt künftig Klartext in
  eine der verschlüsselten Spalten.
- **Transparenz:** Chatverlauf und Eingabe-Historie zeigen weiter das
  Original; das Ausführungsprotokoll wie bisher redigiert.

## 7. Tests

Geheimnis: `Geheim-0096`. Wo der Redactor beteiligt ist (T1, T2, T6–T11),
in der Form `password=Geheim-0096`; `redact_text` macht daraus
`[REDACTED]` (vor dem Schreiben der Tests im Test selbst festhalten). Sonst
beliebig.

- **T1 Titel:** Die KI (Mock-Provider) antwortet mit einem Titel, der
  `password=Geheim-0096` enthält → gespeicherter Titel enthält das
  Geheimnis nicht. Scheitert heute.
- **T2 Titel nur Platzhalter:** Titel `password=Geheim-0096` allein →
  kein Titel gespeichert (`title IS NULL`).
- **T3 Rohdatei (A3):** wie beschrieben → kein Treffer in keiner Datei.
- **T4 Gegenprobe (A4):** Ablauf aus T3 mit durchreichendem Cipher →
  Suche findet das Geheimnis.
- **T8 Notizvorschlag im Chat (A2):** KI (Mock) schlägt eine Notiz mit
  `password=Geheim-0096` vor → das Ereignis an die Oberfläche und nach
  Bestätigung die gespeicherte Revision enthalten das Geheimnis nicht.
  Scheitert heute.
- **T9 Notizvorschlag beim Trennen (A2):** derselbe Fall über den Weg
  beim Trennen der Sitzung; geprüft werden das Ereignis **und** die
  gespeicherte Revision. Scheitert heute.
- **T11 Notizvorschlag über MCP (A2):** derselbe Fall über einen
  MCP-Aufruf → Ergebnis an den Client, Ereignis und gespeicherte Revision
  ohne Geheimnis. Scheitert heute.
- **T12 „In Notiz übernehmen" bleibt (A2, Gegenfall):** Der Nutzer
  übernimmt eine Chatzeile mit `password=Geheim-0096` → die Revision
  enthält sie unverändert.
- **T10 Nur Muster (A2):** Vorschlag besteht nur aus dem Geheimnis → kein
  Vorschlag.
- **T6 Kürzen (adversarial):** Titel mit 55 Zeichen Text und danach
  `password=Geheim-0096` (Muster über Position 60 hinweg) → kein
  Bruchstück des Geheimnisses im gespeicherten Titel.
- **T7 Titel mit mehreren Mustern** und Geheimnis in Anführungszeichen →
  kein Treffer.
- **T5 Wächter:** Die bestehenden Store-Tests auf verschlüsselte BLOBs
  bleiben unverändert grün.

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

- **K1 — T6 prüft andere Längen als in §7 genannt.** Mit 55 Zeichen Fülltext
  beginnt `password=` an Position 55; das Kürzen auf 60 Zeichen schneidet
  bereits im Schlüsselwort, der Geheimniswert erreicht den Titel nie. Der
  Test könnte in dieser Form auch dann nicht scheitern, wenn man Schwärzen
  und Kürzen vertauscht — also genau bei dem Fehler, den er finden soll
  (gemessen). T6 prüft den Spec-Fall weiterhin als Grenzfall, zusätzlich 45
  Zeichen und einen Fall mit **nachlaufendem Anker**
  (`https://u:<wert>@host`, die URL-Regel braucht das `@host` hinter dem
  Wert). Nur der letzte trägt den Gegenbeweis. Verschärfung des Tests, keine
  Änderung an A1. Einzelheiten in ADR 0088, Abschnitt 4.
- **K2 — A3 wird um einen Fall bei offener Datenbank ergänzt.** Ein sauber
  geschlossener Pool hinterlässt keine `-wal`-Datei (gemessen); T3
  durchsucht deshalb faktisch nur die Hauptdatei, und die in „Umsetzung"
  genannte Angriffsrichtung „Rohdatei-Suche, die nur die Hauptdatei liest
  (WAL)" bliebe unbelegt. Ein dritter Test sucht vor dem Schließen, wo
  `-wal` und `-shm` existieren. ADR 0088, Abschnitt 5.
- **K3 — Ein vollständig geschwärzter Notizvorschlag wird gemeldet.** A2
  verlangt nur „kein Vorschlag". Im Chat- und im MCP-Weg wird zusätzlich
  eine feste Meldung ausgegeben (ohne Inhalt der KI); beim Verbindungsende
  bleibt es bei einem Log-Eintrag, weil Spec 0010 §2 Punkt 4 dort
  ausdrücklich „kommentarlos beenden" festlegt. ADR 0088, Abschnitt 2.
- **K4 — §3 („Ziel") ist durch den Redactor begrenzt.** Der Satz „enthält
  keine Datei des Datenbank-Verzeichnisses das Geheimnis im Klartext" gilt,
  soweit die Muster des Redactors greifen — §1 Punkt 5 sagt das bereits für
  den Titel. Ein grüner `strings`-Lauf bei der Handabnahme belegt den
  geprüften Fall, nicht die Aussage in voller Allgemeinheit. ADR 0088,
  Abschnitt 6.

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `fix(app-logic): redact generated chat session titles before storing them [BL-0295]` — A1, T1, T2, T6, T7.
2. `fix(app-logic): redact AI note proposals before showing them [BL-0295]` — A2, T8–T12.
3. `test(persistence): prove no plaintext secret reaches the database files [BL-0295]` — A3, A4, T3–T5.

**Priorität:** ERHÖHT. Angriffsrichtungen für den Review:
- Titel wird vor dem Redactor gekürzt und schneidet ein Muster an (T6).
- Ein Weg, auf dem ein Notizvorschlag entsteht, bleibt ungeschwärzt (T8,
  T9); die geschwärzte Fassung wird angezeigt, aber das Original
  gespeichert (T8 prüft beides).
- Rohdatei-Suche, die nur die Hauptdatei liest (WAL) oder vor dem
  Schließen liest.
- Geheimnis in UTF-16 oder anderer Kodierung: nicht verlangt, aber im
  Bericht nennen, dass nur UTF-8-Bytes gesucht werden.

**Aufteilung:** ein Lauf, Opus (Redaction und Persistenz), klein.

**Berührte Module:** `crates/app-logic/src/orchestration/notes.rs`,
`crates/app-logic/src/orchestration/action_exec.rs` (Notizvorschlag im Chat),
`crates/persistence-sqlite/` (Tests).

**Melde zurück:** Beleg „T1 scheitert gegen den alten Stand"; die Liste
der Dateien, die T3 durchsucht hat.

**Abnahme von Hand (Item BL-0295, durch den Architekten):** App starten,
`mysql -p'geheim'` über den Chat vorschlagen und ausführen, Sitzung
beenden, `strings` über alle Dateien des Datenbankverzeichnisses →
kein `geheim`, außer in Notizen/Regeln nach §3. T3 ersetzt diesen Lauf
nicht, weil er die Schreibpfade der App umgeht.
