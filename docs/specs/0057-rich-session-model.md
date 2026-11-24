# Spec 0057 — Reiches Session-Modell (Aktivitäts-Beobachter, Summary, Kompaktierung)

Status: umgesetzt. Das frühere Ledger und die aufgezeichneten Sitzungen sind entfernt (#1506); Aktivität erreicht Erweiterungen nur noch über Beobachter (#1505).
Zweck: Lange Sitzungen und große Notizen lassen den KI-Kontext nicht mehr anwachsen, bis die Sitzung hängt: Der an die KI gesendete Kontext wird vor jeder Anfrage bei Bedarf verkleinert (Zusammenfassung alter Runden, Kürzung einzelner Ausgaben, verkürzte Notiz), während registrierte Beobachter alle Vorgänge der Sitzung vollständig und redigiert als Ereignisse erhalten.
Bezüge: Spec 0004 (Persistenz und Migrationen), Spec 0034 (Chat-Sitzungen), Spec 0036 (Chat-Inhalte), Spec 0101 (verschlüsselte Datenbank), Spec 0039 (Fencing), Spec 0051 (Rate-Limit-Handling), Spec 0024 (Sprache), Spec 0003 und 0023 (Notiz-Vorschläge, Diff-Bestätigung), Spec 0058 und 0079 (Notiz-Oberfläche), ADR 0003 und 0004 (Notiz-Scopes), ADR 0161 (kein Protokoll im Kern; ersetzt ADR 0047 und 0160), ADR 0048 (Token-Schätzung, Kontextaufbau), ADR 0049 (Summary-Fallback), ADR 0050 (Notiz-Kürzungs-Dialog).
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
3. **Kein Protokoll im Kern:** Kommandos, Entscheidungen, Ergebnisse,
   KI-Nachrichten und Dateizugriffe der KI gehen **redigiert** als Ereignisse
   an registrierte Beobachter (§1). Der Kern speichert sie nicht.
4. **Summary:** Eigener KI-Aufruf beim Kompaktieren, **mit Fallback**: bei
   Ausfall werden die Runden abgeschnitten, die Sitzung hängt nie.
5. **Migration:** Bestehende Chats bleiben unverändert erhalten; das alte
   Ledger wird entfernt (§5).

## 1. Aktivitäts-Beobachter (kein Protokoll im Kern)

Der Kern führt **kein eigenes Protokoll** der Sitzungsaktivität: Es gibt keine
Tabelle dafür, keine Aufbewahrung, keinen Lösch- und keinen Lesezugriff
(Issue #1506, ADR 0161). Wer Aktivität festhalten will, registriert als
Erweiterung einen **Beobachter** (Issue #1505); was er damit speichert, liegt
in seiner Verantwortung.

### 1.1 Was ein Beobachter erhält

Pro Ereignis: Zeitstempel, die Sitzung (Kennung der laufenden Verbindung,
Art `mcp` oder `terminal`; jede Nutzer-Sitzung beginnt als `terminal`, auch
wenn zu ihr ein Chat gehört; die Chat-Kennung trägt sie erst ab „Chat
begonnen"), die **Quelle** (`user`, `ai` oder
`mcp-agent`; bei `mcp-agent` nennt der Sitzungsbeginn den MCP-Client), das
Kennzeichen **Redaction unvollständig** und den Inhalt. Typen:

- **Sitzungsbeginn / -ende** — Server-Schnappschuss (Name, Host, Port,
  Benutzer, Gruppen, Tags) und Zeitzone des Geräts; beim Ende der Grund.
- **Chat begonnen** — kommt genau einmal je Chat-Sitzung mit der ersten
  Chat-Nachricht des Nutzers und nennt die Chat-Sitzung; alle späteren
  Ereignisse tragen deren Kennung, die früheren keine. „Neuer Chat“ beginnt
  eine neue Chat-Sitzung: Ereignisse danach tragen keine Kennung, bis die
  erste Nachricht des neuen Chats wieder „Chat begonnen“ erzeugt. Eine
  Wiederverbindung oder eine fortgesetzte Chat-Sitzung erzeugt kein zweites
  „Chat begonnen". Eine Sitzung ohne Chat-Sitzung (lokaler Pseudo-Server)
  erzeugt es nie.
- **Kommando vorgeschlagen** — das Kommando; die Quelle ist die Herkunft des
  Vorschlags. Ein im Bestätigungsdialog vom Nutzer bearbeiteter Text ist ein
  eigener Vorschlag mit Quelle `user`.
- **Entscheidung** — bestätigt, abgelehnt oder per Regel automatisch
  freigegeben; mit Grund, Code und, falls eine Regel ausschlaggebend war,
  dieser Regel (Muster, Wirkung, Geltungsbereich) samt Herkunft. Ein Klick im
  Bestätigungsdialog hat die Quelle `user`. Entscheidet die Filter-Engine
  allein oder läuft die Bestätigungsfrist ab (Code `TIMEOUT`), ist die Quelle
  die Herkunft des ursprünglichen Vorschlags.
- **Kommando ausgeführt** — das Kommando und das Ergebnis (stdout, stderr,
  Exit-Code, ob die Ausgabe gekürzt oder das Kommando abgebrochen wurde),
  **redigiert**.
- **KI-Nachricht** — Antworttext der KI.
- **Datei gelesen** — Pfad, Größe und SHA-256, nie der Dateiinhalt.
- **Datei geschrieben** — Pfad, Pfad der Sicherungskopie, Größe und SHA-256
  vorher/nachher, ob Sudo genutzt wurde; nie Inhalt und nie ein Diff. Ein
  abgelehnter oder fehlgeschlagener Schreibvorgang erzeugt kein „Datei
  geschrieben"-Ereignis; die Entscheidung dazu ist über dieselbe
  Aktions-Kennung verknüpft.
- **Datei-Schreiben fehlgeschlagen** — Aktions-Kennung, Pfad (redigiert) und
  stabiler Fehlercode, wenn ein Schreibvorgang begonnen hatte und scheiterte
  (auch mit Sudo-Fallback); nie Inhalt. Scheitert schon die Vorbereitung
  (kein SFTP-Kanal, Stat), entsteht kein solches Ereignis.
- **Verbindung** — `connected`, `disconnected` (Grund: `user`, `network` für
  jeden unerwarteten Verlust, `app_exit`), `reconnected` und `failed` mit dem
  Schritt, an dem ein Verbindungsversuch scheiterte (`preparation`,
  `transport`, `session_setup`), ohne Fehlertext und ohne Geheimnisse. Ein
  gescheiterter Versuch hat keine Sitzung: sein Ereignis trägt die Kennung des
  Versuchs und den Server, aber keine Chat-Sitzung. Abgebrochene Versuche und
  der lokale Pseudo-Server melden kein `failed`; ein fehlgeschlagener
  Wiederverbindungsversuch meldet nichts. Eine bereits verlorene Verbindung
  meldet beim Schließen kein zweites `disconnected`.
- **Nutzeraktivität** — höchstens einmal pro Minute und Sitzung, wenn in dieser
  Minute eine Eingabe stattfand (Terminal-Taste, Chat-Nachricht, Bestätigung)
  oder ein vom Nutzer gestarteter Befehl lief. Ein Befehl, den ein MCP-Client
  gestartet hat, zählt nicht als Nutzeraktivität; Eingaben des Nutzers
  während eines solchen Befehls zählen weiterhin. Ohne jeden Inhalt, auch ohne Anzahl; getippte Zeichen
  werden nie gespeichert, ausgewertet oder in ein Ereignis gelegt. Die
  Berechnung läuft neben dem Eingabepfad (nur ein Merker), die Tastatur wird nie
  aufgehalten.

Vorschlag, Entscheidung, Ausführung und Dateizugriff derselben Aktion tragen
dieselbe Aktions-Kennung. MCP-Aktivität erscheint mit Quelle `mcp-agent`,
unabhängig davon, dass MCP-Aktionen nicht in die wiederaufnehmbare
Chat-Historie gehen (Spec 0034).

### 1.2 Redaction — Pflicht

**Jeder Text eines Ereignisses wird redigiert, bevor ihn ein Beobachter
erhält** — mit derselben Redaction wie der KI-Kontext. Sie passiert an der
einen Stelle, die Ereignisse erzeugt, nicht bei den Aufrufern. Konnte die
Redaction der Sitzung nicht vollständig aufgebaut werden, trägt jedes Ereignis
das Kennzeichen „Redaction unvollständig". Entscheidungs-Ereignisse tragen nur
Texte der Filter-Engine, keine Serverinhalte.

### 1.3 Zustellung

Die Zustellung blockiert nie einen Befehl: Ereignisse laufen über eine
begrenzte Warteschlange an einen eigenen Arbeitsfaden. Ist ein Beobachter zu
langsam, gehen Ereignisse verloren (gezählt, ohne Inhalt protokolliert); ein
Beobachter, der abstürzt, stört weder die Sitzung noch andere Beobachter.
Ohne registrierten Beobachter wird kein Ereignis gebaut, und es ändert sich
nichts. Das Sitzungsende trägt einen Grund: „geschlossen" (ausdrücklich
geschlossen) oder „App beendet". Beim Beenden der App erhält ein Beobachter für
jede noch offene Sitzung, für die ein Sitzungsbeginn gemeldet wurde, ein
Sitzungsende mit dem Grund „App beendet" (Umsetzung: #1532, #1539); eine Sitzung,
die noch verbindet, erhält keins. Sobald das Beenden begonnen hat, wird auch kein
Sitzungsbeginn mehr gemeldet: eine Sitzung, die erst dabei fertig verbindet,
erzeugt weder Beginn noch Ende, sodass nie ein Beginn ohne Ende zurückbleibt (#1545). Die Reihenfolge beim Beenden: zuerst enden die
Aufgaben aller Sitzungen, dann folgen die Sitzungsenden, zuletzt wird die
Zustellung geleert. Ein Ereignis einer Sitzung kommt daher nie nach deren
Sitzungsende; nach dem Sitzungsende wird kein weiteres Ereignis dieser Sitzung
mehr eingereiht. Das Beenden insgesamt bleibt bei etwa 6 s: die Aufgaben der
Sitzungen bekommen 2 s, die Sitzungsenden 1 s und die Zustellung ihre eigenen
3 s, auch wenn eine Sitzung nicht endet. Bereits eingereihte Ereignisse werden
noch zugestellt, sodass ein blockierender Beobachter das Beenden nicht länger
aufhält. Eine Chat-Sitzung wird beim Verwerfen ohne Nachricht samt ihrer Zeile
entfernt; nichts bleibt zurück.

### 1.4 Frühere Protokolldaten

Frühere Versionen hielten ein Ledger und „aufgezeichnete Sitzungen" in der
Datenbank. Beim Update auf diese Version werden beide Tabellen und ihre
Verweise aus den Chat-Sitzungen entfernt (Migration 0030); Chats, Nachrichten,
Summaries und Notizen bleiben unverändert.

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
  ein (sie bildet Chat-Kontinuität ab; der Audit-Verlauf geht an die Beobachter).
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
dieselbe Information: wie viele frühere Runden entfernt wurden. Der Hinweis
macht keine Aussage darüber, wo der Verlauf sonst verbleibt. Der Text der Summary
selbst wird nicht übersetzt. Ohne Sprachwechsel bleiben die Hinweise von
Anfrage zu Anfrage unverändert; gespeicherte Historie enthält
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

**Echte Token-Zahlen (Issue #601).** Meldet der Provider nach einer Antwort
die tatsächliche Eingabegröße (Eingabe-Token plus Cache-Lese- und
Cache-Schreib-Anteil), merkt sich die Sitzung sie. Die Schätzung der
nächsten Anfrage wird mit dem beobachteten Verhältnis „echt / geschätzt“
korrigiert — das Verhältnis liegt nie unter 1,0 (die Korrektur senkt die
4-Byte-Schätzung also nie) und wird über mehrere Antworten geglättet.
Kompaktiert wird, sobald die korrigierte Schätzung 75 % des Fensters
erreicht. War die zuletzt gemeldete echte Größe bereits über 75 %,
kompaktiert die nächste Anfrage in jedem Fall, was auch die Schätzung sagt.
Das gilt für Anthropic ebenso wie für OpenAI-kompatible Anbieter (Chat Completions, Responses, Ollama), sofern sie Usage melden; die gemeldete Größe ist dabei immer die volle Prompt-Größe (Cache-Anteile werden weder doppelt gezählt noch weggelassen). Ohne Usage-Angabe des Providers gilt die reine Schätzung. Der Zustand liegt
nur im Speicher der Sitzung.

**Anzeige im Chat.** Nach jeder Antwort zeigt der Chat in der Kopfzeile
(neben „AI · <Modell>“ und „Neuer Chat“) die zuletzt gemeldete Eingabegröße
und das Fenster als Balken aus 8 Segmenten (gefüllt: aufgerundeter Anteil
× 8) samt Zahlen, z. B. „120k / 200k“ — ab 75 % in der Warnfarbe; vor der
ersten Antwort erscheint nichts; ein Tooltip erklärt, dass ältere Teile
automatisch zusammengefasst werden. Es werden nur Zahlen angezeigt, nie
Inhalte. Wurden für eine Anfrage ältere Teile zusammengefasst oder gekürzt,
erscheint einmal pro Kompaktierungsschritt (nicht je Anfrage) der Hinweis
„Ältere Teile dieses Chats wurden zusammengefasst, damit sie in den Kontext
des Modells passen.“ Gespeicherte Historie und Notizen bleiben
unverändert.

**Neuer Chat (Issue #648).** „Neuer Chat“ setzt die gemerkte Größe und das
beobachtete Verhältnis zurück: die Anzeige zeigt sofort „0“ (Fenstergröße
unverändert), und die Kompaktierung des neuen Chats nutzt nicht das
Verhältnis des alten. Nach der ersten Antwort gilt wieder der echte Wert.
Ein fortgesetzter gespeicherter Chat zeigt den Wert des vorigen Chats nicht;
bis zu seiner ersten Antwort ist keine Größe bekannt.

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

Die **Ereignisse** für Beobachter enthalten immer alles. Die Kompaktierung betrifft nur die an
den Provider gesendete Kopie des Kontexts; weder die im Frontend gezeigte
vollständige Historie noch die gespeicherte Notiz noch die Ereignisse werden je
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

Die Umstellung (vorwärts, Spec 0004) entfernt das Ledger, die aufgezeichneten
Sitzungen und deren Verweis aus den Chat-Sitzungen. Jede bestehende
Chat-Sitzung, jede Nachricht, jede Summary und jede Notiz bleibt unverändert
(gleiche Kennung, Reihenfolge, Inhalt und Zeitstempel). Das Entfernen der
Protokolldaten ist beabsichtigt und nicht umkehrbar.

## 6. Sicherheitszusagen

- **Ereignisse sind redigiert, bevor ein Beobachter sie erhält** (§1.2); der
  Kern legt davon nichts auf der Platte ab.
- **Kompaktierung betrifft nur den KI-Kontext, nie die Ereignisse.**
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
- Die Migration bricht keine bestehende Sitzung und verliert keine Zeile.

## 7. und 8. (entfallen)

Die früheren Abschnitte „Testbarkeit" (7) und „Reihenfolge der Umsetzung"
(8) waren Test- und Aufgabenplan; sie sind entfallen. Die Nummern werden
nicht neu vergeben.

## 9. Grenzen

- **Notiz-Änderungsvorschläge der KI erzeugen keine Ereignisse**
  (Kommandos, Entscheidungen, Ergebnisse, KI-Nachrichten und Dateizugriffe
  dagegen vollständig).
- **Der Kern hat keine Oberfläche und keinen Export für Aktivität**; das ist
  Sache einer Erweiterung.
- **Zustellung ist bestmöglich:** Bei vollem Puffer gehen Ereignisse
  verloren (§1.3).
- **Die Token-Schätzung ist eine Faustregel**, kein Tokenizer, und das
  Kontextfenster eine Namens-Heuristik. Selbst gehostete Modelle haben
  tatsächlich oft ein anderes Fenster.
- **Untergrenzen:** Drei Runden, 20 000 Byte je Ausgabe und mindestens
  2 000 Byte der server-spezifischen Notiz bleiben immer. Reicht auch das
  nicht (sehr kleines Fenster), wird der Request trotzdem gesendet und nur
  protokolliert.
- **Lokaler Pseudo-Server und rein MCP-getriebene Sitzungen** speichern keine
  Chat-Sitzung (§1.1), daher keine gespeicherte Summary (MCP-Aktionen gehen
  trotzdem an die Beobachter, §1.1); die
  Kompaktierung läuft dort trotzdem im Speicher.
- **Der Banner des Servers** (Betriebssystem-Information) steht in der
  ersten Runde und kann so indirekt in eine gespeicherte Summary gelangen;
  er ist zeichenbeschränkt und kein Geheimnis.
