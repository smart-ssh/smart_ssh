# ADR 0058: Entscheidungen bei der Umsetzung von Spec 0067 (erhöhter Dateibrowser + Meldungen)

Status: Angenommen
Bezug: docs/specs/0067-sftp-elevated-and-toasts.md, Commits `7e4e262`,
`235f9a6`, `d28e266`, `8bfc9a2`, Review-Nacharbeit `030150d`

## 1. Neuer, kleiner Toast-Host statt „bestehendem 0058-Mechanismus“

Die Spec verweist für Teil B auf einen app-weiten Toast aus Spec 0058. Den
gibt es nicht: 0058 enthält keinen Toast-Mechanismus, im Code gab es nur die
zwei speziellen Notiz-Karten (`NoteSuggestionToast`,
`NoteShrinkSuggestionToast`) und einen lokalen 2-Sekunden-Hinweis im
Dateibrowser. **Entscheidung:** ein kleiner Bus (`toastBus.ts`, Muster wie
`navigationBus.ts`) plus `ToastHost` an der App-Wurzel, in der Optik der
Notiz-Karten, unten links, damit er nicht mit ihnen kollidiert. Der lokale
Hinweis im Dateibrowser ist darin aufgegangen. Andere Bereiche können den
Bus künftig mitnutzen.

## 2. Trennung KI/MCP vom erhöhten Kanal: Zugangsnachweis zur Compile-Zeit

Der erhöhte Kanal ist ein eigenes Session-Feld (`elevated_sftp`), getrennt
vom normalen `sftp`, das auch KI-Aktionen nutzen. `ElevatedSftpSlot::lock`
verlangt ein `commands::BrowserAccess`, dessen privates Feld es nur im
Modul `commands` konstruierbar macht (plus `for_tests()` unter
`#[cfg(test)]`). `orchestration`, `mcp_backend` und das MCP-Crate können
den Kanal damit nicht ansprechen. Ein Regressionstest prüft zusätzlich zur
Laufzeit, dass KI- und MCP-`read_remote_file` bei aktivem erhöhtem Kanal
den normalen nutzen.

**Bekannte Grenze (bewusst nicht behoben):** Der Nachweis ist in ganz
`commands.rs` konstruierbar, nicht nur in den Browser-Commands. Die
Browser-Commands in ein eigenes Untermodul zu verschieben (nur dort ist
`BrowserAccess` konstruierbar) wäre die stärkere Form. Das ist aber ein
Umbau von rund 1000 Zeilen, der nichts an der heutigen Sicherheit ändert.
Er ist als Folgeschritt vorgemerkt.

## 3. Jede erhöhte Aktion ist an den erwarteten Ziel-Nutzer gebunden

Das Frontend schickt pro Aufruf `elevated_user` statt nur „erhöht ja/nein“.
Läuft der aktive erhöhte Kanal als jemand anderes (z. B. Datei als
`www-data` zur Bearbeitung geöffnet, später als `root` neu eingeschaltet),
scheitert die Aktion. Ist er weg, scheitert sie ebenfalls. Es gibt nie
einen stillen Rückfall auf den normalen Kanal oder einen anderen Nutzer
(Review-Fund).

## 4. Kanal pro Session statt pro Browser-Ansicht

Die Spec sagt „pro Dateibrowser-Ansicht“. Heute gibt es genau eine Ansicht
pro Session, also ist beides gleichwertig. Der Kanal lebt in der Session:
Er verschwindet mit Trennung und Neustart, und das Panel schließt beim
Öffnen einen evtl. übrig gebliebenen Kanal.

## 5. Audit über strukturierte Log-Zeilen

Spec 0054 verlangt „audit-erfassbar“. Das wurde dort als architektonische
Disziplin gelöst (benannte Commands, ADR 0045). Für den erhöhten Modus
schreibt die App zusätzlich eine `tracing`-Zeile pro server-verändernder
Aktion: Aktion, Pfad(e), `ok`, `elevated`, Ziel-Nutzer, Quelle `manual`.
Nie Dateiinhalte. Protokolliert wird auch ein Fehlschlag, weil ein
rekursives Löschen oder chmod mittendrin scheitern kann, sowie das Ein- und
Ausschalten des Modus. Eine zentrale Audit-Schicht bleibt ein späteres
Feature.

## 6. Probe-Kommandos laufen ohne Filter-Engine

Pfad-Erkennung (`awk … sshd_config`, Test auf `-x`), `env LC_ALL=C sudo -n
-l <pfad>` und `sudo -n <pfad>` laufen direkt über den Transport, nicht über
Filter-Engine oder Chat. Sie sind fest vorgegeben bzw. streng validiert und
werden nur durch einen Klick des Nutzers ausgelöst, wie jede andere
manuelle Browser-Aktion (Spec 0054, Sicherheitsmodell). Ihre Ausgabe (auch
sudo-stderr) geht nur in die Fehlermeldung des Browsers, nie in den Chat
oder den KI-Kontext.

## 7. Härtung über die Spec hinaus

- Der Override bzw. erkannte Pfad muss auf `sftp-server` enden. So lässt
  sich kein `/bin/sh` mit sudo starten und keine NOPASSWD-Regel dafür
  anzeigen.
- Die sudoers-Zeile endet mit `""`: `sftp-server` darf nur ohne Argumente
  laufen, genau so startet die App ihn. Bei Logins mit
  sudoers-Sonderzeichen zeigt die App keine Zeile statt einer falschen.
- Downloads im erhöhten Modus und Temp-Dateien des Bearbeiten-Flows werden
  direkt mit 0600 angelegt.
- Das Timeout umfasst den ganzen Öffnungsvorgang. `russh-sftp` wartet sonst
  auch auf einem schon geschlossenen Kanal bis zu 10 s pro Anfrage; für den
  Handshake gilt deshalb 5 s.

## 8. Bewusst NICHT behoben / offen

- **Risiko-Klassifizierer:** Die NOPASSWD-Regel gilt für alles unter
  diesem Login, auch für KI-vorgeschlagene Kommandos (z. B. SFTP-Pakete per
  Pipe an `sudo -n …/sftp-server`). Die App erreicht den Kanal nicht, die
  Regel selbst aber schon. **Stefans Entscheidung steht aus**, ob Aufrufe
  von `sftp-server` im Risiko-Klassifizierer fest als Rot gelten sollen
  (sicherheitskritisches Modul, nur Verschärfung). Der Warntext neben der
  sudoers-Zeile sagt das jetzt ausdrücklich.
- **Besitzer-Prüfung des Pfads:** Die App prüft nicht, ob `sftp-server` root
  gehört bzw. für den Login unbeschreibbar ist. Zeigt `sshd_config` auf ein
  vom Login austauschbares Binary, würde die angebotene Regel dieses
  freigeben. Der Fall ist selten und der Nutzer trägt die Regel selbst ein;
  vorgemerkt.
- **Nicht-POSIX-Login-Shell (csh/tcsh):** Die sudo-Prüfung nutzt `env` und
  funktioniert dort. Die Pfad-Probe nutzt POSIX-sh-Syntax und meldet dort
  fälschlich „sftp-server nicht gefunden“. Das scheitert zur sicheren Seite;
  der Override im Profil hilft. `Include`-Dateien und klein geschriebene
  `subsystem`-Einträge in `sshd_config` werden nicht ausgewertet (dann
  greift die Liste bekannter Pfade).
- **Toter Kanal nach unerwartetem Verbindungsabbruch:** Der Slot kann bis
  zum Session-Ende `Some` bleiben. Das Frontend beendet den Modus beim
  Status-Ereignis, eine neue Verbindung erzeugt eine neue Session mit leerem
  Slot, und jede Aktion auf einem toten Kanal scheitert sichtbar.
- **Remote-Prozess nach dem Ausschalten:** Nicht verifiziert ist, ob das
  Verwerfen der SFTP-Session den `sftp-server`-Prozess auf dem Server sofort
  beendet oder erst beim SSH-Disconnect. Der Kanal ist aus Sicht der App in
  jedem Fall nicht mehr erreichbar.
- **KI/MCP-Regressionstest:** Er ist ein Zukunftsschutz und wäre auch vor
  der Änderung grün gewesen, weil die Orchestrierung den Kanal nie kannte.
  Sein Gegenbeweis ist deshalb ein absichtlich eingebauter Fehler
  (erhöhter Kanal in den KI-Pfad übernommen), bei dem er scheitert.

## 9. A4 (anderer Nutzer) umgesetzt

Das kam mit dem Mechanismus trivial mit (`sudo -n -u <nutzer>`, Nutzername
streng validiert). Die UI ist ein kleines Feld neben dem Umschalter,
Standard `root`.
