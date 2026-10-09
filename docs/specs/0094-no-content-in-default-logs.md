# Spec 0094 — Keine Inhalte in der Logdatei auf dem Standard-Level

Status: umgesetzt
Zweck: Die Logdatei enthält auf dem Standard-Level (`info`) keinen Kommandotext, keine Kommando-Ausgabe, keinen Chat-, Notiz- oder Prompt-Inhalt und keine Werkzeug-Argumente — unabhängig davon, welche Muster der Redactor kennt.
Bezüge: Spec 0016 (Logging), Spec 0063 (Diagnose-Export), Spec 0095 (Redactor), ADR 0086.

## 1. Grundsatz

Log-Zeilen auf `info`, `warn` und `error` tragen nur inhaltsfreie Angaben:
Kennungen, Entscheidung, gegriffene Regel, Längen, Exit-Code, Fehlercodes,
Zählwerte. Der Inhalt, den diese Zeilen sonst tragen würden, erscheint nur
auf `debug` und läuft dort vorher durch den Redactor. `debug` ist ohne
`RUST_LOG` aus.

Ein Kommando wie `mysql -p'geheim'` — vom Nutzer getippt, von der KI
vorgeschlagen, ausgeführt, abgelehnt, fehlgeschlagen oder über MCP
ausgeführt — hinterlässt `geheim` in keiner Zeile der Logdatei, solange
`RUST_LOG` nicht gesetzt ist. Einzige Ausnahme ist A1.5.

## 2. Ziel

Siehe §1: Keine Zeile der Logdatei enthält ab `info` Inhalt, auch nicht
redigiert.

## 3. Grenzen

- Nur die Logdatei. Datenbank-Inhalte (Ausführungsprotokoll, Chatverlauf,
  Eingabe-Historie) sind nicht Gegenstand dieser Spec.
- Bestehende Logdateien älterer Versionen bleiben unberührt und werden nach
  14 Tagen gelöscht.
- Fehlertexte anderer Typen (Datenbank, Dateisystem, Schlüsselbund) sind
  nicht erfasst; die Panic-Zeile bleibt unverändert.
- Der Redactor erkennt nicht jedes Geheimnis (Spec 0095 §5); die
  Logdatei verlässt sich darauf ab `info` gerade nicht.

## 4. Anforderungen

**A1 — Kein Inhalt ab `info`.** Keine Log-Zeile auf `info`, `warn` oder
`error` enthält, weder roh noch redigiert: Kommandotext; Kommando-Ausgabe;
Dateiinhalt; Chat-, Notiz- oder Prompt-Text; den Systemkontext; Argumente
eines Werkzeugaufrufs (roh oder geparst); den Fehlertext eines KI- oder
SSH-Fehlers. Was jede Zeile behält:

- A1.1 Filter-Entscheidung: Entscheidung, gegriffene Regel bzw.
  Hard-Blacklist-Eintrag, Länge des Kommandos.
- A1.2 Ausgehende KI-Anfrage: Anfrage-Kennung, Anzahl der Verlaufseinträge,
  Namen der Aktionen; je Verlaufseintrag nur Art und Länge (bei Ergebnissen
  Exit-Code und Ausgabelängen).
- A1.3 Werkzeugaufruf: Anfrage-Kennung, Werkzeugname, Länge der Argumente,
  Art der Aktion; bei einem Auswertungsfehler der Fehlercode.
- A1.4 Ausführung: Sitzungs-Kennung, Exit-Code, Ausgabelängen,
  Kommandolänge; bei einem Fehlschlag der Fehlercode.
- A1.5 **Ausnahme** Provider-Fehlerantwort: Der Antworttext bleibt auf
  `warn`, wird **erst** durch den Redactor geführt und **dann** auf höchstens
  512 Zeichen gekürzt. Grund: Er wird zur Diagnose von Fehlkonfigurationen
  gebraucht. Spiegelt ein Provider Teile der Anfrage, bleibt das auf 512
  Zeichen begrenzt (Restrisiko). Diese Zeilen können damit auch im
  Diagnosepaket (Spec 0063) stehen, dort ein zweites Mal redigiert.
- A1.6 MCP-Aufruf: Werkzeug, Server, Art des Ergebnisses
  (angenommen/abgelehnt/fehlgeschlagen …), Länge der Zusammenfassung bzw.
  Meldung.
- A1.7 Wo ein KI- oder SSH-Fehler ab `info` geloggt wird — direkt, über einen
  String oder über einen Typ, der ihn enthält —, steht der Fehlercode statt
  des Fehlertexts.

**A2 — Inhalt nur auf `debug`, redigiert.** Für jede Stelle, deren Inhalt A1
entfernt, gibt es eine `debug`-Zeile mit dem Inhalt, der vorher durch den
Redactor läuft. Bestehende Längenbegrenzungen bleiben. Wo kein
Sitzungs-Redactor erreichbar ist, wird der Standard-Redactor mit gleichem
Musterstand genutzt. Die Redaction für `debug` läuft nur, wenn `debug` für
diese Stelle aktiv ist.

**A3 — Standard bleibt `info`.** Ohne `RUST_LOG` ist `debug` für kein
Modul der App aktiv.

**A4 — Nachrichtentexte.** Die Nachricht einer geänderten Zeile bleibt
gleich; die zugehörigen `debug`-Zeilen haben eine eigene Nachricht.

## 5. Keine Ableitungen aus dem Inhalt

Ab `info` gibt es keinen Hash des Kommandos und keinen Programmnamen:
Passwörter haben wenig Entropie, und das erste Wort kann selbst ein
Geheimnis sein (`PGPASSWORD=… psql`).

## 6. Sicherheitszusagen

- Die Logdatei ist ab `info` keine Datensenke für Inhalte; auf `debug` gilt
  die Regel „Redaction vor jeder Datensenke".
- Die Filter-Entscheidung ändert sich nicht, nur ihre Log-Zeile.
- Transparenz gegenüber Nutzer, KI und MCP-Client (Ereignis, Chat,
  Ausführungsprotokoll, Werkzeug-Ergebnis) bleibt unverändert; nur die
  Log-Zeile ändert sich.
- Das Diagnosepaket (Spec 0063) bleibt eine Positivliste harmloser Zeilen und
  wird zusätzlich redigiert.


## 7. Abnahmefälle

Geheimnis in den Fällen: `geheim-0094`, in einer Form, die der Redactor
nicht erkennt (z. B. `mysql -u root -p geheim-0094`, Spec 0095 §5). „Kein
Treffer" heißt: in keiner Zeile ab `info`.

- **T1 Filter-Entscheidung:** ein Kommando mit Geheimnis → kein Treffer;
  Entscheidung und Kommandolänge sind vorhanden.
- **T2 Verkettung:** das Geheimnis in einem Teilkommando einer Verkettung →
  kein Treffer.
- **T3 Mehrzeilig:** ein Heredoc mit dem Geheimnis → kein Treffer in keiner
  Zeile.
- **T4 KI-Anfrage:** Kontext mit Text, Kommando-Ergebnis, abgelehnter Aktion
  und Systemkontext, je mit Geheimnis → kein Treffer; Art und Länge je
  Eintrag sind vorhanden.
- **T5 Werkzeugaufruf:** Fragment, geparste Aktion und ein Auswertungsfehler,
  dessen Fehlertext das Geheimnis enthält → kein Treffer; Fehlercode vorhanden.
- **T6 Ausführung:** Kommando und Ausgabe mit Geheimnis; ein Fehlschlag mit
  einem SSH-Fehler, dessen Text das Geheimnis enthält → kein Treffer.
- **T7 MCP:** Aufruf eines Kommandos mit Geheimnis, erfolgreich und mit
  Ausführungsfehler → kein Treffer; Art des Ergebnisses vorhanden.
- **T8 Provider-Fehler:** (a) Antworttext mit einem vom Redactor erkannten
  Geheimnis → auf `warn`, ohne Treffer; (b) Antworttext mit 2000 Zeichen →
  höchstens 512 Zeichen; (c) ein KI-Fehler mit Geheimnis im Text an einer
  Stelle aus A1.7 → kein Treffer.
- **T9 `debug`-Zeilen:** Auf `debug` erscheint der Inhalt; ein vom Redactor
  erkanntes Geheimnis ist dort ersetzt. Je ein Fall für Filter, Provider,
  Anwendungslogik und MCP.
- **T10 Standardfilter:** Der Standardfilter (ohne `RUST_LOG`) lässt `debug`
  für keines dieser Module durch, `info` schon.
- **T11 Filter unverändert:** Das Verhalten der Filter-Engine ist von der
  Änderung unberührt.
