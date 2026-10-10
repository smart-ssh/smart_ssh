# Spec 0032 — Lokaler Pseudo-Server „Localhost"

Status: umgesetzt
Zweck: Ein immer vorhandener, nicht löschbarer Eintrag „Localhost" in der Serverliste, über den der KI-Copilot (Vorschlag, Filter-Engine, Bestätigung, Ausführung) auf dem eigenen Rechner genutzt werden kann — ohne dass dafür ein SSH-Server laufen muss.
Bezüge: Spec 0002 (Filter-Engine), Spec 0003 (Server-Profile, Notizen), Spec 0005 (Transport), Spec 0007 (Kernschleife), Spec 0008 (Server-Formular), Spec 0017 (Tabs), Spec 0020 (Dateizugriff), Spec 0026 (Risiko-Anzeige), Spec 0033 (Serverliste), Spec 0043 und Spec 0044 (Ausgabegrenze), ADR 0026, ADR 0030.

## 1. Ziel

Der Nutzer kann den KI-Chat samt Filter-Engine, Risiko-Anzeige und Bestätigung
auf der **eigenen Maschine** ausprobieren und benutzen, ohne einen lokalen
`sshd` einzurichten. Gerade unter macOS und Windows läuft standardmäßig
keiner; die Funktion wäre sonst für die meisten unbenutzbar. Auch der
Einstieg ohne eigenen Server nutzt das: „Localhost" steht oben in der
Serverliste und öffnet eine Sitzung auf diesem Rechner mit denselben
Bestätigungen wie auf einem Server.

## 2. Ausführung statt SSH-zu-sich-selbst

Localhost spricht **kein** SSH-Protokoll. Kommandos, Terminal und Dateizugriff
laufen direkt auf dem lokalen Rechner:

- **Einzelkommando.** Das Kommando läuft über die Shell des Systems
  (`sh -c` unter macOS und Linux, `cmd /C` unter Windows) im
  Home-Verzeichnis des Nutzers; ist dieses nicht verfügbar, im
  Arbeitsverzeichnis der App. Die Standardeingabe ist geschlossen: ein
  Kommando, das auf Eingabe wartet (`cat`, `read`), endet sofort oder
  scheitert, statt zu hängen. Standard- und Fehlerausgabe sowie der Exit-Code
  werden wie bei einem Server eingesammelt. Die Ausgabe ist auf 2 MiB
  begrenzt (Spec 0043, Spec 0044); wird die Grenze erreicht, wird der Prozess
  beendet und das Ergebnis als abgeschnitten markiert — der Exit-Code kann
  dann fehlen.
- **Terminal.** Ein lokales Pseudo-Terminal mit der Standard-Shell des Nutzers
  (`$SHELL`, sonst `/bin/sh`; unter Windows `powershell.exe`). Größenänderung
  wird wie bei SSH unterstützt.
- **Dateibrowser.** Der Dateizugriff (Spec 0020) arbeitet direkt auf dem lokalen
  Dateisystem; relative Pfade gelten ab dem Home-Verzeichnis.
- **Verbindungsaufbau.** „Verbinden" ist sofort erfolgreich. Es gibt keinen
  Handshake, keine Zugangsdaten, keinen Host-Key und keine Host-Key-Abfrage.

Alles oberhalb davon — Filter-Engine, Risiko-Einstufung, Bestätigungsdialoge,
KI-Anbindung, MCP-Server — ist unverändert, weil es nur gegen die
gemeinsame Transport-Schnittstelle arbeitet (Spec 0005, Abschnitt 2).

## 3. Identität und Persistenz

- **Kein Datensatz.** Localhost steht nicht in der Server-Tabelle. Der Eintrag
  wird zur Laufzeit gebildet und trägt eine fest reservierte, nie vergebene
  Kennung (die Nil-UUID). Er ist dadurch nicht löschbar, nicht verschiebbar und
  braucht keine Migration.
- **Immer da und zuerst.** Die Serverliste enthält ihn immer als erstes Element,
  unabhängig von jedem Gruppenfilter. Er gehört nie zu einer Gruppe.
- **Name und Anzeige.** Der Name ist fest „Localhost". Die Zeile zeigt
  `Benutzer@Host` ohne Port, weil es keinen gibt.
- **Nur Notizen und Tags sind editierbar.** Name, Host, Port, Benutzer,
  Anmeldeart, Jump-Host und Startverzeichnis gibt es für Localhost nicht; das
  Formular zeigt sie nicht an, sondern nur einen Hinweis, Notizen und Tags.
  Die Notizen gehen wie bei jedem Server in den KI-Kontext (Spec 0003), nur
  ohne Gruppen- und Vererbungsanteile; die
  KI-Vorschläge zum Aktualisieren oder Kürzen der Notiz beim Beenden (Spec 0010)
  gelten auch hier.
- **Keine Notiz-Historie.** Anders als bei einem Server gibt es für Localhost
  nur den aktuellen Stand der Notiz, keine Versionen und kein Zurücksetzen.
  Notizen und Tags liegen in den App-Einstellungen (ADR 0026).
- **Kein gespeicherter Chat-Verlauf.** Eine Localhost-Sitzung legt keine
  gespeicherte Chat-Sitzung an und ist nicht wiederaufnehmbar (ADR 0030).
- **Feste Eskalationsstufe.** Die Stufe für Folgeaktionen nach gelesenem
  Serverinhalt (Spec 0039) ist für Localhost fest „Balanced" und nicht
  einstellbar (ADR 0026).
- **Kein Verbindungstest.** Das Formular hat keine Schaltfläche „Verbindung
  testen"; ein Aufruf dafür wird abgelehnt.
- **Direkte Änderungen werden abgelehnt.** Bearbeiten, Löschen und Verschieben
  über den normalen Weg schlagen für Localhost mit einer Fehlermeldung fehl.

## 4. Verhalten in der Kernschleife

Localhost durchläuft **dieselbe** Sitzungsstruktur, dieselbe Filter-Engine, dieselben
Bestätigungsdialoge und dieselben Risiko-Anzeigen wie jeder Server — ohne
Verzweigung nach der Kennung in der Sicherheitslogik. Eine Regel mit Geltungsbereich
„dieser Server" wirkt für Localhost wie für jeden anderen Server.

Es gibt **keine** automatische Lockerung für lokale Kommandos: die eigene
Maschine verdient nicht weniger Kontrolle als ein entfernter Server, eher mehr
(eigene Dateien, Zugangsdaten unter `~/.ssh`, `~/.aws`).

## 5. Oberfläche

- Localhost steht fest **oberhalb** der Gruppenhierarchie (Spec 0033) in
  eigenem Rahmen, nie in einem Ordner. Er lässt sich weder ziehen noch ist er
  Ablageziel (Spec 0103).
- Ein Klick öffnet einen Tab wie bei jedem Server (Spec 0017); auch hier gibt
  es höchstens einen Nutzer-Tab.
- Im Server-Formular zeigt Localhost einen Hinweis („Lokaler Pseudo-Server —
  Kommandos laufen direkt auf diesem Rechner, keine SSH-Verbindung. Nur
  Notizen und Tags sind editierbar.") sowie Notizen und Tags, aber weder
  Löschen noch Verbindungstest noch Gruppen- oder Jump-Host-Auswahl.
- Ist sonst kein Server angelegt, verweist der Einstiegs-Block auf Localhost
  als Möglichkeit, ohne Server auszuprobieren (Spec 0069).
- Ein Localhost-Eintrag zählt nie als „angelegter Server".

## 6. Ausschlüsse und Grenzen

- **Kein Jump-Host.** Localhost kann nicht als Jump-Host für andere Server
  dienen: es ist der Ausgangspunkt, keine Zwischenstation. Die Auswahl im
  Formular bietet ihn nicht an; wird er trotzdem gesendet, wird das mit
  „Der lokale Pseudo-Server kann nicht als Jump-Host verwendet werden"
  abgelehnt, bevor irgendein Zugangsdatum berührt wird.
- **Anzeigename fest.** „Localhost" lässt sich nicht umbenennen.
- **Kein Abbruch langer Kommandos.** Der Abbruch eines nie endenden Kommandos
  (Spec 0027) greift für Localhost nicht; das Kommando endet erst von selbst
  oder an der Ausgabegrenze.
- **Kein Sudo-Passwort.** Für Localhost lässt sich kein Sudo-Passwort
  hinterlegen; das Verfahren aus Spec 0018 greift nicht.
- **Windows-Terminal.** Das lokale Pseudo-Terminal unter Windows hängt von der
  Unterstützung der verwendeten Terminal-Bibliothek für ConPTY ab; einzelne
  moderne ConPTY-Flags setzt sie nicht (ADR 0026). Das Verhalten kann deshalb
  von dem unter macOS und Linux abweichen.
