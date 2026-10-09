# Spec 0088 — Bestätigungswarten und Dateiaktionen ohne Panic-Pfade

Status: umgesetzt
Zweck: Der Tab-Hinweis „wartet auf Bestätigung" und der Eintrag der wartenden Bestätigung werden auf jedem Weg aus dem Warten abgeräumt, und die Orchestrierung enthält keinen `unwrap`/`expect`, den ein neuer Aufrufer unbemerkt erreichen kann.
Bezüge: Spec 0002 (Entscheidung Confirm), Spec 0028 (MCP: Tool-Timeout), Spec 0085 (normaler SFTP-Kanal), Spec 0092 (Rot verlangt Bestätigung), Spec 0090 (Clippy-Gate), ADR 0082.

## 1. Ausgangslage

Wartet ein Vorschlag auf die Bestätigung des Nutzers, gibt es zwei Dinge, die
aufgeräumt werden müssen: den Hinweis, dass die Sitzung auf eine Bestätigung
wartet (im Tab sichtbar, abgelesen aus dem Sitzungs-Snapshot), und den Eintrag
der wartenden Bestätigung, über den ein späteres „Ausführen"/„Ablehnen" die
Aktion erreicht. Der Eintrag wird schon registriert, bevor Vorschau und
Zweitmeinung abgewartet werden, damit ein früher Klick nicht verloren geht.

Wird der Warte-Vorgang abgebrochen (der wartende Task wird fallen gelassen),
bliebe ohne diese Spec der Hinweis gesetzt und der Eintrag bestehen. Kein
Produktivpfad bricht den Vorgang heute ab: Der Chat-Turn wartet außerhalb jedes
Abbruchs, und der MCP-Pfad startet den Vorschlag bewusst als eigenen Task. Das
Problem ist, dass neue Aufrufer es unbemerkt auslösen könnten.

Die Orchestrierung enthielt außerdem `unwrap`/`expect`-Aufrufe im
Produktivcode (Bestätigungspfad, SFTP-Dateiaktionen, ein Sperren-Zugriff im
Chat-Verlauf), die heute durch die Programmstruktur nicht auslösbar sind, deren
Sicherheit aber nur im Kommentar oder im Typ des Aufrufers steht.

## 2. Ziel und Nicht-Ziele

Ziel: Das Warten auf eine Bestätigung räumt Hinweis und Eintrag auf **jedem**
Ausgang ab: Entscheidung, Zeitüberschreitung, geschlossener Kanal, Panic und
Abbruch. Die Orchestrierung enthält keinen `unwrap`/`expect` im
Produktivcode, und ein neuer fällt im Gate auf.

Nicht-Ziele:

- Kein workspaceweiter Abbau von `unwrap`/`expect` und kein Lint außerhalb der
  Orchestrierung.
- Kein Wechsel der Mutex-Implementierung, keine neue Abhängigkeit.
- Keine Änderung der Wartezeit-Grenze, an „Zeitüberschreitung = Ablehnung" oder
  am MCP-Verhalten bei einem Tool-Timeout (die Bestätigung in der Oberfläche
  läuft danach weiter, Spec 0028).
- Ein Stopp des Chat-Turns bricht ein laufendes Bestätigungswarten weiterhin
  **nicht** ab.
- `unreachable!` für nicht erreichbare Zweige (MCP kann kein Dokument
  erzeugen) bleibt; der Lint betrifft nur `unwrap`/`expect`.

## 3. Anforderungen

**A1 — Bestätigungswarten räumt auf jedem Weg ab**

- A1.1 Nach jedem Ausgang des Wartens ist der Hinweis „wartet auf
  Bestätigung" der Sitzung wieder aus: nach Genehmigung, Ablehnung,
  Zeitüberschreitung, gedropptem Sender, einem Panic im Wartepfad und nach
  Fallenlassen des wartenden Futures.
- A1.2 Wird der Future zwischen der Registrierung der Bestätigung und ihrer
  Auflösung fallen gelassen (auch schon während Vorschau oder Zweitmeinung), ist
  der Eintrag für diese Aktion nicht mehr registriert. Ein späteres Auflösen
  dafür liefert einen Fehler und führt nichts aus.
- A1.3 Kein Ausgang aus A1.1/A1.2 führt die Aktion aus. Abgeräumt heißt nie
  genehmigt.
- A1.4 Der Bestätigungspfad enthält keinen `expect`/`unwrap`. Dass im
  `Confirm`-Zweig ein Empfänger vorliegt, folgt aus dem Aufbau des Codes und
  nicht aus einer Laufzeitprüfung.
- A1.5 Das bisherige Verhalten bleibt: Zeitüberschreitung = Ablehnung mit dem
  Grund „Zeitüberschreitung"; gedroppter Sender = nichts ausführen und
  „frühere Ablehnung" setzen; ein MCP-Tool-Timeout lässt die Bestätigung in der
  Oberfläche weiterlaufen.

**A2 — Vergiftete Sperre**

- A2.1 Ist die Sperre des Hinweises vergiftet, panicken weder der
  Sitzungs-Snapshot noch das Setzen oder Abräumen im Bestätigungspfad. Sie
  arbeiten mit dem enthaltenen Wert weiter. Der Wert ist einfach und nach jeder
  Zuweisung in sich stimmig.
- A2.2 Das Abräumen aus A1.1/A1.2 panickt nie, auch nicht bei vergifteter Sperre
  des Hinweises oder des Eintragsverzeichnisses, und auch nicht während eines
  laufenden Panics. Ein Panic beim Abräumen während des Abwickelns bricht den
  Prozess ab.

**A3 — SFTP-Dateiaktionen ohne Panic-Pfad**

- A3.1 Die Dateiaktionen (Datei lesen, Datei schreiben inklusive Backup und
  Sudo-Rückfall) enthalten keinen `expect`/`unwrap` auf den normalen SFTP-Kanal.
- A3.2 Wäre der Kanal nach dem Öffnen trotzdem leer, endet die Aktion mit einem
  Aktionsfehler im Chat (mit Fehlercode), nie mit einem Panic. Diese Umwandlung
  „leerer Kanal → Fehler" liegt an **einer** Stelle.
- A3.3 Keine Änderung an Spec 0085 A3: Der normale Kanal wird weiterhin nur aus
  dem Transport der eigenen Sitzung befüllt und lässt sich von außerhalb des
  Anwendungsmoduls weder ersetzen noch leeren.

**A4 — Weitere Stelle und Absicherung**

- A4.1 Die Sperre der Herkunfts-Kennzeichen des Chat-Verlaufs (MCP) verhält sich
  bei Vergiftung wie A2.1, sowohl beim Schreiben einer Verlaufsnachricht als auch
  beim Lesen in der Kompaktierung (ein Panic dort ließe den Verlauf leer
  zurück). Verlauf und Kennzeichen bleiben gleich lang.
- A4.2 Das Clippy-Gate (Spec 0090) lehnt einen neuen `unwrap`/`expect` im
  Produktivcode der Orchestrierung ab. Testcode ist ausgenommen; die Ausnahme
  wird ausdrücklich an den Testmodulen gesetzt, damit sie denselben Radius hat
  wie der Lint (ADR 0082, Abschnitt 5).
- A4.3 Wo die Absicherung nur per Kommentar möglich ist, steht die Invariante
  am Typ bzw. Feld, nicht am Aufrufer.

## 4. Verhaltensdetails

- Das Abräumen hängt an einem Wert, dessen Freigabe es erledigt, nicht an
  Anweisungen von Hand vor und nach dem Warten. Er beginnt mit der Registrierung,
  nicht erst beim Setzen des Hinweises.
- Der Hinweis wird nur dann gelöscht, wenn dort noch die **eigene** Aktion
  steht. MCP und Chat teilen eine Sitzung, und der hängende Hinweis einer
  anderen wartenden Aktion bleibt stehen.
- Die Schnittstelle des Eintragsverzeichnisses bleibt unverändert.

## 5. Sicherheitszusagen

- **Eine Zeitüberschreitung lehnt ab, sie gewährt nie** (A1.3, A1.5).
- **KI und MCP erreichen nur, was durch Filter und Bestätigung läuft:** Der
  Umbau betrifft nur das Warten. Ein abgeräumter oder schon aufgelöster Eintrag
  führt nichts aus.
- **Nie hängen:** Der Hinweis bleibt nicht mehr stehen (A1.1).
- **Paarung Transport und normaler Kanal** (Spec 0085 A3): A3.3. Die
  Übersetzungsfehler-Prüfungen an den Sperr-Funktionen bleiben unverändert grün.
- Redaction, Ledger, Kompaktierung und Rate-Limit sind nicht berührt.

## 6. Prüffälle

Alle Tests haben eine Zeitgrenze, damit ein Hänger rot wird statt zu
blockieren.

- **T1 (A1.1).** Aktion mit Confirm vorschlagen, warten bis der Hinweis gesetzt
  ist, den wartenden Task abbrechen. Erwartet: Hinweis aus, der Snapshot meldet
  keine wartende Aktion.
- **T2 (A1.2).** Wie T1; zusätzlich ist der Eintrag nicht mehr registriert.
- **T3 (A1.3).** Nach T1 die Aktion mit Genehmigung auflösen: Das liefert einen
  Fehler, und es wird kein Kommando ausgeführt.
- **T4 (A1.2, adversarial).** Abbruch **während** der Zweitmeinung (Provider mit
  hängendem Stream, Hinweis noch aus). Erwartet: Eintrag nicht mehr registriert,
  Hinweis aus.
- **T5 (A2.1, adversarial).** Die Sperre des Hinweises vergiften. Danach laufen
  Snapshot, Vorschlag und Genehmigung ohne Panic. Die Aktion wird genau einmal
  ausgeführt, danach ist der Hinweis aus.
- **T6a (A2.2, adversarial).** Sperre vergiften, dann ein Panic nach der
  Registrierung in einem eigenen Task. Erwartet: Der Task endet als Panic, kein
  Prozessabbruch, der Eintrag ist nicht mehr registriert.
- **T6b (A2.2, adversarial).** Sperre vergiften, warten bis der Hinweis gesetzt
  ist, den Task abbrechen. Erwartet: Der Task endet als Abbruch, nicht als Panic,
  und der Hinweis ist aus.
- **T6c (A2.2).** Die interne Sperre des Eintragsverzeichnisses vergiften.
  Danach panicken Registrieren, Auflösen und Abbrechen nicht und verhalten sich
  wie ohne Vergiftung.
- **T7 (A4.1).** Die Sperre der Herkunfts-Kennzeichen vergiften, dann eine
  Nachricht pushen und kompaktieren. Kein Panic, Verlauf und Kennzeichen bleiben
  gleich lang, der Verlauf ist nach der Kompaktierung nicht leer.
- **T8 (A1.5).** Die bestehenden Tests zu Ablehnung, Zeitüberschreitung und
  Folgerunde bleiben grün.
- **T9 (A1.5, MCP).** Der bestehende Test, dass ein Tool-Timeout die Bestätigung
  in der Oberfläche nicht abräumt, bleibt grün.
- **T10 (A1.3).** Genehmigung, danach sofort eine zweite Auflösung derselben
  Aktion: genau eine Ausführung, die zweite liefert einen Fehler, der Hinweis
  ist aus.
- **T11 (A3.1/A4.2).** Ein `unwrap()` in einer Produktivfunktion der
  Orchestrierung lässt das Clippy-Gate rot werden; derselbe `unwrap()` in einer
  Testdatei nicht.
- **T12 (A3.3).** Die `compile_fail`-Doctests an den Sperr-Funktionen für Sitzung
  und Transport bleiben grün.
- **T13 (A3.2).** Scheitert das Öffnen des SFTP-Kanals bei Datei lesen/schreiben,
  endet die Aktion mit einem Aktionsfehler mit Code und ohne Panic.
- **T14 (A1.1, adversarial).** **Während** des Wartens (vor jeder Antwort) ist
  der Hinweis gesetzt und der Eintrag registriert. Danach genehmigen, und die
  Aktion läuft. Das schließt ein Abräumen aus, das schon vor der Antwort
  stattfindet.
- **T15 (A3.2).** Die eine Stelle, die einen leeren Kanal in einen Fehler
  umwandelt, liefert an einer frischen Sitzung (ohne geöffneten Kanal) einen
  SSH-Fehler und keinen Panic.
