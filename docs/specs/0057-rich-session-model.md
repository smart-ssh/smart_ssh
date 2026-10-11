# Spec 0057 — Reiches Session-Modell (Ledger, Summary, Kompaktierung)

Status: umgesetzt
Zweck: Lange Sitzungen und große Notizen lassen den KI-Kontext nicht mehr anwachsen, bis die Sitzung hängt: Der an die KI gesendete Kontext wird vor jeder Anfrage bei Bedarf verkleinert (Zusammenfassung alter Runden, Kürzung einzelner Ausgaben, verkürzte Notiz), während ein dauerhaftes, redigiertes Ledger alle Vorgänge der Sitzung vollständig festhält.
Bezüge: Spec 0004 (Persistenz und Migrationen), Spec 0034 (Chat-Sitzungen), Spec 0036 (Chat-Inhalte), Spec 0101 (verschlüsselte Datenbank), Spec 0039 (Fencing), Spec 0051 (Rate-Limit-Handling), Spec 0024 (Sprache), Spec 0003 und 0023 (Notiz-Vorschläge, Diff-Bestätigung), Spec 0058 und 0079 (Notiz-Oberfläche), ADR 0003 und 0004 (Notiz-Scopes), ADR 0047 (Ledger), ADR 0048 (Token-Schätzung, Kontextaufbau), ADR 0049 (Summary-Fallback), ADR 0050 (Notiz-Kürzungs-Dialog).
Review-Priorität: ERHÖHT (Redaction, Persistenz, Migration)

## Entscheidungen

1. **Auslöser der Kompaktierung:** Eine Größengrenze (proaktiv, etwa drei
   Viertel des Kontextfensters des Modells, **nach dem Fencing** gerechnet),
   die letzten drei Runden bleiben immer voll erhalten, und einzelne
   übergroße Ausgaben werden gesondert gekürzt.
2. **Notiz:** Beim Kompaktieren wird sie **verkürzt gesendet**, die
   **gespeicherte Notiz bleibt vollständig**. Scope-priorisiert
   (server-spezifisch bleibt am längsten), und erst als letztes Mittel nach
   Chat-Verlauf und Ausgaben. Bei großen Notizen schlägt die Anwendung am
   Verbindungsende eine dauerhafte Kürzung vor, die der Nutzer bestätigt.
3. **Ledger:** Kommandos, Entscheidungen, Ergebnisse und KI-Nachrichten,
   **redigiert vor dem Schreiben**, nur anhängend, nie zusammengefasst.
4. **Summary:** Eigener KI-Aufruf beim Kompaktieren, **mit Fallback**: bei
   Ausfall werden die Runden abgeschnitten, die Sitzung hängt nie.
5. **Migration:** Keine Daten-Migration. Additive Schema-Änderung, alte
   Sitzungen bleiben unberührt.

## 1. Das Ledger

Ein **nur anhängendes Protokoll** der Vorgänge einer Sitzung — die dauerhafte
Wahrheit, aus der später Audit-Log und Report schöpfen können. Es wird nicht
von der Kompaktierung berührt (§3.3).

### 1.1 Was hineinkommt

Pro Eintrag: Zeitstempel, Sitzung, eine in der Sitzung monoton wachsende
Reihenfolge, **Quelle** (`user`, `ai` oder `mcp-agent`), Typ und Inhalt:

- **Kommando vorgeschlagen** — das Kommando; die Quelle ist die Herkunft des
  Vorschlags (KI oder MCP-Agent). Ein im Bestätigungsdialog vom Nutzer
  bearbeiteter Text ist ein eigener Vorschlag mit Quelle `user`.
- **Entscheidung** — bestätigt, abgelehnt oder per Regel automatisch
  freigegeben; mit Grund, Code und, falls eine Regel ausschlaggebend war,
  dieser Regel samt Herkunft. Ein Klick im Bestätigungsdialog hat die Quelle
  `user`. Entscheidet die Filter-Engine allein (automatische Freigabe oder
  Blockade, auch bei erneuter Prüfung eines bearbeiteten Textes), oder läuft
  die Bestätigungsfrist ab (Code `TIMEOUT`), ist die Quelle die Herkunft des
  ursprünglichen Vorschlags: ein Mensch hat dann nicht entschieden.
- **Kommando ausgeführt** — das Kommando, das Ergebnis (stdout, stderr,
  Exit-Code, ob die Ausgabe gekürzt oder das Kommando abgebrochen wurde),
  **redigiert**.
- **KI-Nachricht** — Antworttext der KI, auch ein erzeugtes Dokument.

Das Ledger erfasst auch MCP-Aktivität (Quelle `mcp-agent`), unabhängig davon,
dass MCP-Aktionen nicht in die wiederaufnehmbare Chat-Historie gehen
(Spec 0034).

### 1.2 Redaction — Pflicht

**Jeder Ledger-Eintrag wird redigiert, bevor er geschrieben wird** — mit
derselben Redaction wie der KI-Kontext. Der Ledger liegt dauerhaft auf der
Platte; ohne Redaction wäre er eine Klartextsammlung sensibler Ausgaben.
Die Redaction passiert an der einen Schreibstelle, nicht bei den Aufrufern,
damit ein Aufrufer sie nicht vergessen kann. (Entscheidungs-Einträge tragen
nur Texte der Filter-Engine, keine Serverinhalte.)

### 1.3 Verschlüsselung

Das Ledger liegt wie die Chat-Historie in der verschlüsselten
Datenbankdatei (Spec 0101, Spec 0036 §1); der Inhalt steht dort ohne eigene
Feldverschlüsselung. Wo diese Spec „verschlüsselt" sagt (Ledger, Summary),
ist dieser Schutz gemeint.

### 1.4 Persistenz

Eigene Tabelle, die additiv angelegt wird. Einträge werden nie geändert oder
gelöscht, außer zusammen mit der ganzen Sitzung (Löschen der Sitzung löscht
ihr Ledger). Zwei gleichzeitige Schreibvorgänge derselben Sitzung (Chat und
MCP) bekommen nie dieselbe Reihenfolgenummer.

Das Ledger gehört zur gespeicherten Chat-Sitzung: Wird für eine Verbindung
keine Chat-Sitzung gespeichert (Chat-Historie aus, rein MCP-getriebene
Sitzung, lokaler Pseudo-Server, Anlegefehler), entstehen auch keine Ledger-Einträge.

## 2. Die Summary

Wenn die Kompaktierung alte Runden aus dem KI-Kontext entfernt, ersetzt eine
**Zusammenfassung** sie, damit die KI die Kontinuität behält.

### 2.1 Erzeugung

- Ein **eigener KI-Aufruf** über den Haupt-Provider der Sitzung bittet um
  eine bündige, rein faktische Zusammenfassung (was wurde getan, welcher
  Stand, welche offenen Punkte). Der Aufruf läuft mit Rate-Limit-Handling
  (Spec 0051), dem Inaktivitäts-Timeout des Providers und einem äußeren
  Zeitrahmen von 120 Sekunden; er hat kein Werkzeug-Schema.
- Die neue Summary fasst die **bisherige Summary und die jetzt neu zu
  entfernenden Runden** zusammen (rollierend, nicht jedes Mal von vorn).
  Deckt die gespeicherte Summary die zu entfernenden Runden schon ab, wird
  sie ohne neuen Aufruf wiederverwendet.
- Eingehendes wird vor dem Senden zusätzlich redigiert (wie vor jeder
  Anfrage); die zurückkommende Zusammenfassung wird ebenfalls redigiert und
  auf höchstens 8 000 Byte begrenzt, damit sie nicht jede weitere
  Kompaktierung dominiert.
- Nachrichten, die von MCP-Aktionen stammen, fließen nicht in die Summary
  ein (sie bildet Chat-Kontinuität ab; der Audit-Verlauf bleibt im Ledger).
  Die Runden selbst werden trotzdem wie jede andere entfernt.

### 2.2 Fallback — kritisch

Schlägt der Summary-Aufruf fehl (Rate-Limit trotz Wiederholung, Timeout,
Fehler, leere Antwort), **darf die Sitzung nicht hängen oder abbrechen**.
Die ältesten Runden werden dann **ohne** Summary abgeschnitten, mit einem
sichtbaren Hinweis im Kontext („ältere Konversation gekürzt"). Die zuletzt
gültige gespeicherte Summary bleibt für den nächsten Versuch erhalten. Die
Summary ist eine Verbesserung; ihr Ausfall blockiert nie die Grundfunktion.

### 2.2a Sprache der Hinweise im Kontext

Der Kürzungshinweis (§2.2) und die Hülle, in der eine Summary im Kontext
steht, sind auf Deutsch und Englisch vorhanden und folgen der Sprache des
System-Prompts derselben Anfrage (Spec 0024, Abschnitt 2: gespeicherte
UI-Sprache, sonst System-Locale, sonst Englisch). Beide Fassungen tragen
dieselbe Information: wie viele frühere Runden entfernt wurden und dass der
vollständige Verlauf im Session-Ledger erhalten bleibt. Der Text der Summary
selbst wird nicht übersetzt. Ohne Sprachwechsel bleiben die Hinweise von
Anfrage zu Anfrage unverändert; gespeicherte Historie und Ledger enthalten
sie nicht, sie entstehen bei jeder Anfrage neu.

### 2.3 Persistenz

Die aktuelle Summary wird mit der Sitzung gespeichert (in der
verschlüsselten Datenbankdatei, Spec 0101) und beim Fortsetzen der Sitzung
wieder geladen; die Zahl der von ihr abgedeckten Runden wird dabei auf die
tatsächlich geladene Historie begrenzt. Das Speichern ist bestmöglich: ein
Fehler wird protokolliert und bricht die Anfrage nicht ab.

## 3. Kompaktierung

### 3.1 Auslöser

**Vor jeder Anfrage an den Provider** wird die Größe des Requests geschätzt:
etwa 4 Byte je Token, **nach dem Fencing** gerechnet (`<`, `>`, `&` werden
dort zu 4–5-Zeichen-Entitäten, rohe Zeichen würden systematisch
unterschätzen), einschließlich System-Prompt und Werkzeug-Schemas.
Überschreitet die Schätzung **75 %** des Kontextfensters des Modells,
wird kompaktiert, bevor das harte Limit kommt (der Rest ist Puffer für die
Antwort).

Das Kontextfenster wird aus dem konfigurierten Modell bestimmt: 200 000 Token
für Anthropic, 128 000 für OpenAI (16 000 für gpt-3.5) und konservative
32 000 für alle übrigen (OpenAI-kompatible, lokale Modelle, unbekannte
Namen) — im Zweifel zu klein angenommen, nie zu groß.

### 3.2 Ablauf (Reihenfolge der Kürzung)

Kompaktiert wird in dieser Reihenfolge, jeweils nur so weit wie nötig, bis
der Request unter der Grenze liegt:

- **3.2.1 Alte Runden** werden durch die Summary ersetzt (§2). Eine Runde
  beginnt mit einer Nutzer-Nachricht und reicht bis zur nächsten. Die
  **letzten drei Runden** bleiben immer voll erhalten.
- **3.2.2 Einzelne übergroße Ausgaben** in den erhaltenen Runden werden auf
  je 20 000 Byte für stdout und stderr gekürzt, ältere zuerst; die Runde
  bleibt erhalten. Die Kürzung wird als „Ausgabe abgeschnitten" **außerhalb**
  des Ausgabe-Blocks gekennzeichnet — ein Hinweis im Block wäre von echter
  Ausgabe fälschbar (Spec 0039).
- **3.2.3 Die Notiz wird verkürzt gesendet** (§4.1) — erst als letztes
  Mittel.

### 3.3 Was nicht kompaktiert wird

Das **Ledger** behält immer alles. Die Kompaktierung betrifft nur die an
den Provider gesendete Kopie des Kontexts; weder die im Frontend gezeigte
vollständige Historie noch die gespeicherte Notiz noch das Ledger werden je
verändert. Die Ersatzstellen (Hinweis, Summary) gehören nur zu dieser einen
Anfrage.

Eine geladene (fortgesetzte) Sitzung wird beim Laden nicht gekürzt; die
Kompaktierung greift erst vor dem Senden.

## 4. Notiz-Handling

### 4.1 Verkürzt senden (automatisch, verlustfrei für die gespeicherte Notiz)

Kommt die Kompaktierung bis zur Notiz (§3.2.3), geht eine **gekürzte
Fassung an die KI**; die gespeicherte Notiz bleibt unangetastet.
Priorisiert nach **Scope**: Notizen allgemeinerer Gruppen werden zuerst
entfernt, die server-spezifische bleibt am längsten und wird nie ganz
entfernt, sondern höchstens auf einen Rest (mindestens 2 000 Byte) gekürzt.
Das gekürzte Ende trägt den Hinweis, dass die gespeicherte Notiz vollständig
ist. Die Kürzung ist lautlos und unterbricht die Arbeit nicht mit Dialogen.

### 4.2 Vorschlag zur dauerhaften Kürzung (am Verbindungsende, Nutzer entscheidet)

Ist die gespeicherte Server-Notiz **groß** (mindestens 10 000 Zeichen),
erscheint beim **Verbindungsende** ein Dialog: „Deine Notiz für diesen
Server ist sehr groß und kann bei langen Sitzungen gekürzt werden müssen.
Soll ich sie zusammenfassen?" mit **„Ja, zusammenfassen"**, **„Mache ich
selbst"** (öffnet die Notiz-Bearbeitung) und **„Später"**.

- Pro Verbindungsende erscheint höchstens ein Notiz-Dialog: macht die
  Anwendung am Verbindungsende ohnehin einen Notiz-Aktualisierungs-
  Vorschlag, hat dieser Vorrang; der Kürzungs-Vorschlag kommt dann beim
  nächsten Verbindungsende, falls die Notiz groß bleibt. Die Prüfung ist
  eine reine Größenprüfung, ohne KI-Aufruf.
- „Ja" löst erst jetzt einen KI-Aufruf aus (nie automatisch). Er fasst die
  **gespeicherte** Notiz zusammen (Ergebnis höchstens 4 000 Byte, wird
  redigiert; Rate-Limit-Handling und äußerer Zeitrahmen von 120 Sekunden wie
  bei der Summary) und zeigt das Ergebnis im **Diff-Bestätigungsdialog**
  (Spec 0003/0023): Der Nutzer sieht, was die neue Notiz wird, bevor sie
  ersetzt wird. Scheitert der Aufruf, erscheint eine Fehlermeldung.
- **Kein stilles Kürzen der gespeicherten Notiz** — immer Nutzer-Bestätigung.
  Übernehmen speichert genau den angezeigten Inhalt, Ablehnen lässt die
  Notiz unverändert.
- Wurde die Zusammenfassung vom Provider wegen seines Längenlimits
  **abgeschnitten**, erscheint der Vorschlag trotzdem, aber der Dialog zeigt
  über dem Diff eine deutliche Warnung („Die Zusammenfassung wurde
  abgeschnitten und ist unvollständig. Prüfe sie sorgfältig oder lehne sie
  ab."). Die Warnung stammt von der App, steht außerhalb des Diff-Inhalts und
  kann vom KI-Text weder vorgetäuscht noch verdeckt werden. Ist die
  Zusammenfassung zusätzlich länger als erlaubt, bleibt der Kappungs-Hinweis
  am Textende zusätzlich erhalten. Andere Notiz-Vorschläge (Chat,
  Verbindungsende) zeigen diese Warnung nie.
- Der Dialog funktioniert für jeden Server gleich, auch für den lokalen
  Pseudo-Server. Ein sanfter Hinweis schon beim Bearbeiten einer großen
  Notiz ist in Spec 0079 beschrieben und nutzt denselben Schwellwert.

## 5. Migration

**Keine Daten-Migration.** Neue Tabelle (Ledger) und zusätzliche Spalten
(Summary) werden **additiv** angelegt (vorwärts, datenerhaltend, Spec 0004).
Alte Sitzungen bleiben **unberührt und lesbar**; sie haben kein Ledger und
keine Summary. Nur neue (und fortgesetzte, ab da) Sitzungen führen das
Ledger. Kein Parsen oder Umschreiben alter Daten.

## 6. Sicherheitszusagen

- **Ledger redigiert vor dem Persistieren** — kein Klartext-Secret
  dauerhaft auf der Platte.
- **Ledger liegt in der verschlüsselten Datenbankdatei** (§1.3).
- **Kompaktierung betrifft nur den KI-Kontext, nie das Ledger.**
- **Die gespeicherte Notiz wird nie ohne Nutzer-Bestätigung verändert**
  (das verkürzte Senden ist verlustfrei; die dauerhafte Kürzung braucht den
  Diff-Dialog).
- **Summary-Ausfall blockiert die Sitzung nie** (Fallback: Runden
  abschneiden).
- Summary- und Notiz-Kürzungs-Aufruf unterliegen Rate-Limit-Handling,
  Provider-Timeout und äußerem Zeitrahmen; sie sind nie ungeschützt.
- Eine Summary wird wie jeder KI-Inhalt behandelt: redigiert und nicht
  privilegiert; Hinweise der Anwendung stehen außerhalb von
  Ausgabe-Blöcken.
- Die additive Migration bricht keine bestehende Sitzung.

## 7. und 8. (entfallen)

Die früheren Abschnitte „Testbarkeit" (7) und „Reihenfolge der Umsetzung"
(8) waren Test- und Aufgabenplan; sie sind entfallen. Die Nummern werden
nicht neu vergeben.

## 9. Grenzen

- **Das Ledger erfasst nur Kommandovorschläge** als Aktionstyp. Dateilese-
  und Schreibaktionen sowie Notiz-Änderungsvorschläge der KI erzeugen
  derzeit keine Ledger-Einträge (Kommandos, Entscheidungen, Ergebnisse und
  KI-Nachrichten dagegen vollständig, ADR 0047).
- **Das Ledger hat noch keine Oberfläche:** Es wird geschrieben und
  gelöscht, aber nirgends angezeigt oder exportiert.
- **Ledger-Schreiben ist bestmöglich:** Ein Schreibfehler wird protokolliert
  und bricht die Sitzung nicht ab; der Eintrag fehlt dann.
- **Die Token-Schätzung ist eine Faustregel**, kein Tokenizer, und das
  Kontextfenster eine Namens-Heuristik. Selbst gehostete Modelle haben
  tatsächlich oft ein anderes Fenster.
- **Untergrenzen:** Drei Runden, 20 000 Byte je Ausgabe und mindestens
  2 000 Byte der server-spezifischen Notiz bleiben immer. Reicht auch das
  nicht (sehr kleines Fenster), wird der Request trotzdem gesendet und nur
  protokolliert.
- **Lokaler Pseudo-Server und rein MCP-getriebene Sitzungen** speichern keine
  Chat-Sitzung (§1.4), daher kein Ledger und keine gespeicherte Summary; die
  Kompaktierung läuft dort trotzdem im Speicher.
- **Der Banner des Servers** (Betriebssystem-Information) steht in der
  ersten Runde und kann so indirekt in eine gespeicherte Summary gelangen;
  er ist zeichenbeschränkt und kein Geheimnis.
