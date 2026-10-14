# ADR 0141 — Erweiterungsinhalte in Chat-Nachrichten: opaker Verweis, Versiegelung im Kern

Status: angenommen · ersetzt ADR 0137 · ändert ADR 0138, Entscheidung 4 · Spec 0110, Spec 0038 §2a, Spec 0039, Spec 0006 §8

Umsetzung in Schritten: #504 setzt Entscheidung 1 (Verweis und Speicherung),
2 (Erweiterungspunkt, Signaturen), 7 (Aufräumen), 8 (Verlaufsanzeige,
Platzhalter) und 10 (Migration 0022) um; Versiegelung, Sendeweg und das
Entfernen des Anhang-Modells folgen.

## Kontext

Seit #469/#470 kennt der Kern ein vollständiges Datenmodell für Dateianhänge
(`ChatAttachment`, `AttachmentKind`, Grenzen, Speicherung in
`chat_messages`, Migration 0021) und rendert sie je Anbieter als Bild- oder
Dokumentblock. Der Kern selbst kann aber keine Dateien auswählen; jeder
Anhang stammt aus einer Edition. Damit trägt der Kern ein Feature-Datenmodell,
das er nie selbst füllt, und jede Änderung an Anhängen (neue Typen, andere
Grenzen, andere Speicherung) ist eine Kernänderung.

Gleichzeitig gibt es inzwischen allgemeine Erweiterungspunkte:
erweiterungseigene Tabellen (ADR 0136, Erweiterungs-Migrationen), den
Session-Erweiterungspunkt (ADR 0136, Session extension point) und die
Ausgangs-Transformation (#464). Was fehlt, ist ein allgemeiner Weg, wie eine
Erweiterung **Inhalt an eine Nutzer-Nachricht hängt**, ohne dass der Kern
dessen Datenmodell kennt, und ohne dass die Sicherheitszusagen für alles,
was an die KI geht (Redaction, Fencing, Größengrenzen, Spec 0039/0110),
von der Erweiterung abhängen.

Migration 0021 ist in keiner veröffentlichten Version enthalten (nicht in
v0.6.0). Es gibt also keine Nutzerdaten in der Form `attachments`.

## Entscheidungen

1. **Ein opaker Erweiterungsteil statt eines Anhang-Modells.**
   `MessageContent::Attachments(Vec<ChatAttachment>)` entfällt. Neu ist
   `MessageContent::Extension(ExtensionPart)` mit
   `extension` (Name der registrierten Erweiterung, vom Kern gesetzt, nie von
   der Erweiterung geliefert), `kind` (opak, nur für die Erweiterung),
   `id` (opak, Verweis in den Speicher der Erweiterung), `label` (Anzeigename,
   redigiert), `media` (`text | image | pdf`, vom Kern aus dem aufgelösten
   Inhalt abgeleitet) und `estimated_tokens` (vom Kern berechnet). Der Kern
   speichert nur diesen Verweis im Chatverlauf (`content_type = 'extension'`),
   nie den Inhalt. Je Teil eine eigene Zeile, direkt nach dem Text der
   Nachricht, Rolle `user`.
   *Alternative:* das Anhang-Modell im Kern lassen und nur die Speicherung
   auslagern. Verworfen: der Kern behielte Typen, Grenzen und Namen eines
   Features, das er nicht ausliefert.

2. **Erweiterungspunkt `ChatContentExtension` in `Wiring`** (Liste; jede
   Erweiterung mit eindeutigem Namen aus Kleinbuchstaben und Ziffern).
   Methoden, alle mit Standard ohne Wirkung:
   - `take_pending(session, id) -> Option<PendingContent>`: übersetzt eine
     opake ID aus dem Eingabefeld (nie ein Pfad, ADR 0113) in Label und
     Blöcke. `None` heißt „nicht meine ID“.
   - `persist(session, part_id, blocks: Vec<RedactedBlock>)`: die Erweiterung
     speichert, was der Kern ihr zurückgibt: Text **bereits redigiert**,
     Binärblöcke geprüft. `RedactedBlock` kann nur der Kern bauen.
   - `resolve(session, part) -> Result<Option<Vec<ResolvedBlock>>, …>`: liefert
     den Inhalt für einen Versand. `None` heißt „Inhalt existiert nicht mehr“.
   - `session_removed(session)` und `reconcile(live_sessions)`: Aufräumen
     (Entscheidung 7).
   `ResolvedBlock` ist entweder `Text { text }` oder
   `Binary { mime, bytes, filename }`. Es gibt keinen dritten Weg.

3. **Versiegelung nur im Kern, an genau einer Stelle.** Jeder Versand an
   einen Anbieter (Chat-Turn und Kompaktierung) läuft schon heute durch
   `reapply_redaction_for_send`. Dort ersetzt der Kern jeden
   `Extension`-Teil der Sendekopie durch einen versiegelten Inhalt
   (`SealedContent`): Grenzen prüfen, Text redigieren, dann als
   nicht vertrauenswürdig fencen (Reihenfolge nach ADR 0034), feste
   Sicherheitszeile anhängen, Binärblöcke prüfen (Entscheidung 5).
   `SealedContent` hat private Felder, hat nur einen Konstruktor im Kern,
   ist nicht serialisierbar und kann deshalb weder gespeichert noch von einer
   Erweiterung gebaut werden. Anbieter sehen nur `SealedContent`; ein
   unversiegelter `Extension`-Teil, der trotzdem einen Anbieter erreicht,
   wird dort als feste, gefencte Platzhalterzeile ohne Inhalt gerendert
   (fail closed, nie Inhalt).

4. **Fence-Art aus einer geschlossenen Liste.** Die Erweiterung meldet bei
   der Registrierung ihre Quelle aus einem Kern-Enum (`ExtensionSource`,
   zunächst nur `LocalFile` → `UntrustedKind::LocalFile`). Sie kann kein
   eigenes Tag, keinen eigenen Sicherheitshinweis und keinen ungefencten
   Text liefern. Eine neue Quelle ist eine Kernänderung, weil sie die
   Fence-Marker und den erklärenden Prompttext ändert (Spec 0039; gleiche
   Abwägung wie ADR 0136, Session extension point).
   Jeder `Extension`-Teil setzt die Eskalation nach Spec 0039 §5, auch nach
   dem Fortsetzen, unabhängig von der Quelle.

5. **Binärblöcke generisch nach Medientyp.** Der Kern nimmt nur
   `image/png`, `image/jpeg`, `image/gif`, `image/webp` und
   `application/pdf` an und prüft, dass die ersten Bytes zum gemeldeten Typ
   passen. Alles andere lehnt der Kern ab, bevor ein Anbieter aufgerufen
   wird. Binärbytes werden nie als Text dekodiert, nie redigiert, nie
   protokolliert (Protokoll: Anzahl und Bytes). `ai-providers` kennt statt
   `Image`/`Pdf` nur noch `ContentPart::Binary { mime, filename, base64 }`
   und serialisiert je Anbieter nach Medienklasse (Anthropic `image` /
   `document`, Chat Completions `image_url` / `file`, Responses
   `input_image` / `input_file`; OpenAI-kompatible lokale Server wie Ollama
   laufen über Chat Completions). Die Fähigkeitstabelle aus ADR 0138 bleibt
   inhaltlich gleich, ist aber nach Medienklasse geschlüsselt
   (`accepts(mime_class)`), nicht nach Anhang-Art. `AiError::AttachmentUnsupported`
   wird zu `AiError::InputUnsupported(MediaClass)`; der Fehlercode
   `AI_ATTACHMENT_UNSUPPORTED` bleibt.

6. **Grenzen: allgemein im Kern, fachlich in der Erweiterung.** Der Kern
   erzwingt für alles, was an die KI geht, harte Obergrenzen: höchstens
   5 Erweiterungsteile je Nachricht, höchstens 4 Blöcke je Teil, höchstens
   1 MiB je Block (gemessen an den tatsächlichen Bytes), Text gültiges UTF-8,
   und neu höchstens 20 MiB Binärbytes je Anfrage über den ganzen Verlauf.
   Geprüft wird beim Annehmen **und** bei jedem Versand; ein Verstoß sendet
   nichts. Eine Erweiterung darf strengere Grenzen haben (Dateianzahl,
   Dateigröße, erkannte Typen), nie weitere.

7. **Speicher und Aufräumen gehören der Erweiterung.** Inhalte liegen in
   erweiterungseigenen Tabellen (ADR 0136, Erweiterungs-Migrationen), also in
   derselben verschlüsselten Datenbank und wie Chat-Sitzungen nicht im
   Voll-Backup (Spec 0109 C2). Weil Fremdschlüssel in Kerntabellen dort
   verboten sind, meldet der Kern jede gelöschte Chat-Sitzung über
   `session_removed`: einzelnes Löschen, Aufbewahrungsfrist und Löschen eines
   Servers (die Sitzungs-IDs werden vor dem Löschen gesammelt, der Aufruf
   kommt nach dem erfolgreichen Löschen). Nach dem Start der Erweiterung
   übergibt der Kern einmal die Menge der lebenden Sitzungs-IDs
   (`reconcile`), damit Reste nach einem Absturz verschwinden.

8. **Ohne Erweiterung bleibt der Verlauf lesbar.** Die Historie zeigt einen
   Erweiterungsteil mit Label und Art (`ChatHistoryEntryDto::ExtensionParts`),
   ohne die Erweiterung zu fragen. Ist die Erweiterung nicht registriert oder
   meldet sie `None`, geht statt des Inhalts eine vom Kern formulierte,
   gefencte Zeile „Inhalt nicht verfügbar“ mit dem Label an die KI; der
   Verlauf zeigt denselben Zustand. Meldet die Erweiterung einen Fehler,
   wird nicht gesendet (fail closed).

9. **Oberfläche und Vokabular bleiben.** Der Eingabefeld-Slot, die Chips und
   die Fehlercodes (`ATTACHMENTS_*`, `ATTACHMENT_*`) bleiben unverändert im
   Kern; `send_chat_message` nimmt weiter `attachment_ids`. Der Feature-Name
   `chat_attachments` bleibt Vokabular der Berechtigungen (Spec 0037); der
   Kern prüft ihn weiter nicht.

10. **Migration 0021 bleibt unverändert.** Sie ist nicht veröffentlicht,
    aber Entwicklungs- und Testdatenbanken haben sie angewendet; `sqlx`
    prüft Prüfsummen und fehlende Migrationen und würde solche Datenbanken
    nicht mehr öffnen. Stattdessen: Migration 0022 erlaubt
    `content_type = 'extension'`, Migration 0023 entfernt `attachments` aus
    der Constraint und löscht Zeilen dieser Art (es gibt sie nur in
    Entwicklungsdaten).

## Konsequenzen

- Die Community Edition ändert sich nicht: keine Erweiterung, kein
  Eingabefeld-Knopf, Datenbank nach 0023 gleichwertig zu vorher (eine
  zusätzliche erlaubte Nachrichtenart).
- Der Kern verliert `ChatAttachment`, `AttachmentKind`,
  `ChatAttachmentResolver`, `MAX_ATTACHMENT_*` und die Anhang-Speicherung.
- Jeder Versand löst Erweiterungsteile neu auf (bis 5 MiB je Nachricht aus
  der Erweiterungstabelle). Die Erweiterung darf zwischenspeichern.
- Neue Medientypen oder Quellen sind bewusst Kernänderungen.
- ADR 0137 ist ersetzt; ADR 0138 gilt weiter bis auf Entscheidung 4
  (typisierte Teile jetzt nach Medientyp).
