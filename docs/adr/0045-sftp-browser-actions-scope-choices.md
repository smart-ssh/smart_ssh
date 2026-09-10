# 0045-sftp-browser-actions-scope-choices

## Status
Akzeptiert

## Kontext

Spec 0054 (SSH-/SFTP-Dateibrowser — Aktionen & Menüs) ließ mehrere
Umsetzungsfragen bewusst offen bzw. bot an mehreren Stellen zwei
gleichwertige Wege an ("Cut/Paste ODER Drag-and-Drop", kein konkreter Weg
zur lokalen Änderungserkennung im "Lokal öffnen"-Flow, keine Vorgabe zur
Audit-Erfassbarkeit außer "so bauen, dass sie später hineinfließen
können"). Diese ADR hält die während der Umsetzung getroffenen
Entscheidungen fest.

## Entscheidungen

**1. Drei-Punkte-Menü-Bug (Teil 0): Ursache war eine Event-Race, kein
Rendering-Fehler.**

Der bestehende "Klick-außerhalb-schließt-das-Menü"-Effekt hängte einen
rohen `document.addEventListener("click", ...)` ein, der bei jedem
folgenden Klick unbedingt `setMenuFor(null)` aufrief — auch dann, wenn
dieser Klick eigentlich ein ANDERES Menü öffnen sollte. Da der Trigger-
Klick (setzt den neuen State) und der alte, noch nicht abgeräumte
Listener (setzt `null`) im selben synchronen Klick-Batch liefen, gewann
der zuletzt registrierte Listener — das Menü eines anderen Eintrags ließ
sich nie öffnen, ein bereits offenes schloss bei jedem weiteren
Drei-Punkte-Klick sofort. Behoben durch `stopPropagation()` im Trigger
(React ruft dabei auch `nativeEvent.stopPropagation()` auf, s.
`FileBrowserPanel.tsx`).

**2. Verschieben: Cut/Paste statt Drag-and-Drop-zwischen-Zeilen.**

Die Spec nennt beides als gleichwertige Alternativen ("Cut/Paste ODER
Drag-and-Drop"). Cut/Paste deckt die Anforderung (Verschieben innerhalb
des Servers, mit Überschreib-Warnung bei Zielkollision) vollständig ab,
ohne einen zweiten, komplexeren Interaktionsmechanismus (Drag-Handling
zwischen Tabellenzeilen, Drop-Ziel-Erkennung, visuelles Feedback während
des Ziehens) einführen zu müssen. Reine Umfangs-Reduktion, keine
Funktionslücke gegenüber der Spec.

**3. Lokale Änderungserkennung im "Lokal öffnen"-Flow: Polling statt
nativem Datei-Watcher.**

Für "solange sie in Bearbeitung ist" beobachten wurde bewusst **kein**
plattformübergreifender nativer Dateisystem-Watcher (z. B. die
`notify`-Crate) eingeführt, sondern ein einfaches Polling auf die
Änderungszeit der lokalen Datei (`local_file_mtime`, alle 2 Sekunden vom
Frontend abgefragt, s. `useLocalEditSession.ts`). Begründung: Es wird zu
jedem Zeitpunkt höchstens EINE Datei während einer aktiven Bearbeitung
beobachtet — für diesen schmalen Anwendungsfall ist eine Verzögerung im
Sekundenbereich unauffällig, während ein nativer Watcher eine neue,
plattformspezifisch nicht triviale Abhängigkeit (unterschiedliche Backends
auf macOS/Windows/Linux, zusätzliche Fehlerfälle bei Berechtigungen/
Netzlaufwerken) für einen Nutzen eingeführt hätte, der für diesen
schmalen, kurzlebigen Anwendungsfall nicht im Verhältnis zum Mehraufwand
steht. Bei Bedarf (z. B. wenn Polling sich in der Praxis als zu träge
erweist) ist das ohne API-Bruch nachrüstbar — `useLocalEditSession`s
öffentliche Schnittstelle kennt kein Polling-Detail nach außen.

**4. Audit-Erfassbarkeit: architektonische Disziplin statt eines
Platzhalter-Audit-Logs.**

Die Spec verlangt ausdrücklich nur, dass server-verändernde Aktionen
"so gebaut sind, dass sie später in den Audit-Log fließen können" — der
Log selbst ist explizit kein Teil dieser Spec. Es wurde bewusst **kein**
Stub/Platzhalter-Audit-Mechanismus eingeführt (das wäre spekulativer
Vorgriff auf ein noch nicht spezifiziertes Feature, s. CLAUDE.md: "Don't
add features... beyond what the task requires"). Stattdessen: jede
server-verändernde Aktion (`sftp_chmod`, `sftp_delete`, `sftp_rename`,
`sftp_mkdir`, `sftp_upload`) ist ein eigener, schmal geschnittener,
benannter Tauri-Command, der ausschließlich über `session_sftp()` auf die
Session zugreift — dieselbe Struktur, mit der bereits jetzt jeder dieser
Befehle eindeutig als "manuell" identifizierbar ist (die KI-Aktionen
`ReadRemoteFile`/`WriteRemoteFile` laufen über einen komplett getrennten
Code-Pfad in `orchestration.rs`, nie über diese `sftp_*`-Befehle). Ein
künftiger Audit-Log kann an jedem dieser Befehle ansetzen, ohne dass
vorher ein Refactoring nötig wäre.

**5. Ordner-Download: Zielverzeichnis-Namensgebung.**

Ein heruntergeladener Ordner landet immer als `<Zielverzeichnis>/
<Ordnername>/...`, nie direkt lose im Zielverzeichnis verstreut — sonst
wäre der heruntergeladene Inhalt nach dem Download nicht mehr als "der
eine heruntergeladene Ordner" wiedererkennbar. Gilt sowohl für den
Standard-Download (`sftp_download_default`) als auch für den
Dialog-Download (`sftp_download_dir`).

## Spec-Reviewer-Review des Gesamtpakets (ERHÖHTE Priorität für Teil 3/4)

Der abschließende Review über das ganze Paket (Commits `7c7071e`..`bd59af9`)
fand mehrere sicherheitsrelevante Punkte. **Behoben:**

- Zip-Slip über servergelieferte Dateinamen (`RemoteEntry::name`/`::path`)
  beim rekursiven Ordner-Download und beim "Lokal öffnen"-Download — ein
  (kompromittierter) Server hätte `../../x` oder einen absoluten Pfad als
  Eintragsnamen liefern und damit lokale Dateien außerhalb des
  beabsichtigten Zielverzeichnisses überschreiben können. Neue
  `safe_local_segment`-Prüfung vor jedem `PathBuf::join(entry.name)`.
- Symlink an der WURZEL eines rekursiven Löschens/chmod folgte dem Ziel
  (`stat` statt `lstat`) — ein Symlink auf ein Verzeichnis außerhalb des
  sichtbaren Baums (`/var/www/current -> /etc`) hätte den gesamten
  Baum dahinter gelöscht bzw. dessen Rechte geändert. Neue
  `SftpSession::lstat`-Trait-Methode, `delete_recursive`/
  `chmod_recursive` prüfen die Wurzel jetzt damit statt mit `stat`
  (deckte dabei einen zweiten, vorbestehenden Bug in
  `LocalFileSession::remove` auf: es nutzte `metadata` statt
  `symlink_metadata`, wodurch das Entfernen eines Symlinks selbst mit
  `ENOTDIR` scheiterte).
- `useLocalEditSession.ts`s `buildUploadOffer` versprach im Doc-Kommentar
  "konservativ eine Warnung, wenn ein Zeitstempel fehlt", setzte das aber
  nicht um (`!==` auf zwei `null`-Werten ergibt `false`) — genau der Fall,
  in dem die Remote-Änderungs-Konflikt-Prüfung nichts prüfen konnte, hätte
  still "kein Konflikt" gemeldet.
- Ein Fehlschlag NACH einem erfolgreichen Download im "Lokal öffnen"-Flow
  (z. B. `openPath` findet kein Programm) ließ die bereits heruntergeladene
  Temp-Kopie unaufgeräumt liegen, weil ohne gesetzten Session-State kein
  Weg mehr existierte, sie später zu entfernen.
- `opener:allow-open-path` griff ohne eigenen Scope-Eintrag gar nicht (das
  Plugin verweigert ohne Scope JEDEN Pfad) — "Lokal öffnen" wäre im echten
  Build fehlgeschlagen. Jetzt auf `$CACHE/smart-ssh/edit-sessions/**` mit
  `app: true` eingeschränkt.
- `close_edit_session` nahm jeden vom Frontend übergebenen Pfad an (kein
  Verlust an Funktionalität, aber billige Verteidigung in der Tiefe) — jetzt
  auf den Editier-Temp-Ordner der jeweiligen Session eingeschränkt.
- chmod-Dialog: die numerische Eingabe war auf 3 Ziffern begrenzt und
  verlor damit still das setuid/setgid/sticky-Bit einer Datei bei jeder
  erneuten Eingabe (die Checkbox-Matrix war davon nicht betroffen) — jetzt
  durchgehend 4-stellig.
- `file_name_of` lieferte bei einem abschließenden `/` im Pfad
  (`/srv/data/`) den kompletten Pfad statt nur des letzten Segments zurück.
- Temp-Datei/-Ordner des "Lokal öffnen"-Flows bekommen jetzt `0600`/`0700`
  (Unix) statt der Standard-Umask — der Inhalt ist Remote-Serverinhalt,
  potenziell mit Secrets.

**Bewusst nicht behoben, mit Begründung:**

- **Kein Sweep verwaister Editier-Temp-Ordner beim App-Start.** Aktuell
  räumt nur `close_edit_session` (expliziter Flow-Abschluss) und
  `disconnect()` (Fallback-Netz beim Trennen) auf — ein App-Absturz/
  Force-Quit während einer aktiven Bearbeitung lässt den
  `edit-sessions/<session_id>/`-Ordner dauerhaft liegen. Ein
  Start-Sweep ("alles unter `edit-sessions/` löschen, da beim Start
  ohnehin keine Session aktiv sein kann") wäre einfach nachrüstbar, war
  für diesen ohnehin schon großen Schritt aber nicht mehr enthalten —
  echter offener Punkt für eine spätere Ergänzung.
- **`disconnect()` löscht eine ggf. gerade in einem externen Programm
  offene Bearbeitungskopie ohne gesonderten Hinweis.** Das Frontend hört
  nicht auf das `connection-status-changed`-Event, um den "wird lokal
  bearbeitet"-Flow proaktiv zu beenden/zu warnen — ein Upload nach einem
  Trennen liefe ins Leere (sichtbarer Fehler, aber kein Datenverlust ohne
  Vorwarnung wäre besser). Aufwand für eine für Spec 0054 zusätzliche
  Event-Verdrahtung stand nicht im Verhältnis zu diesem Randfall.
- **Keine Mehrfachauswahl im Dateibrowser-Menü.** Spec 0054, Teil 1 nennt
  das nur "wo sinnvoll" — für dieses Paket wurde durchgehend nur
  Einzelauswahl umgesetzt (deckt alle Kern-Anwendungsfälle ab), eine
  Mehrfachauswahl für z. B. Sammel-Download/-Löschen wäre ein eigener,
  spürbarer UI-Umfang.
- **"Fortschritt bei großen Dateien" bleibt Start/Ende + Gesamtgröße, kein
  echter Byte-Fortschritt.** Ererbtes Verhalten aus Spec 0020 (dort bereits
  bewusst so entschieden, s. `crate::events`-Moduldoc), Spec 0054
  wiederholt die Formulierung, ohne mehr zu verlangen als bereits
  vorhanden.
- **Kosmetische Ungenauigkeiten in Bestätigungsdialogen**, kein
  Datenverlust-Risiko: ein Umbenennen-Kollisionsdialog auf einem
  case-insensitiven Server kann "würde überschrieben" versprechen, obwohl
  der Server je nach Konfiguration anders reagiert; ein Datei-Upload auf
  einen gleichnamigen Remote-ORDNER zeigt denselben "Datei überschreiben"-
  Dialog, obwohl der eigentliche Upload danach mit einem Server-Fehler
  scheitert. Beide Fälle zeigen den Fehler sichtbar, nur der vorherige
  Dialogtext ist nicht immer exakt zutreffend.
- **`FilePropertiesDialog`s Rechte-Anzeige bleibt 3-stellig oktal** (anders
  als der jetzt 4-stellige chmod-Dialog) — rein anzeigend, nicht editierbar,
  daher kein Datenverlust-Risiko wie beim chmod-Dialog, nur eine für eine
  Datei mit setuid/setgid/sticky-Bit unvollständige Anzeige.
- **`FileBrowserPanel.tsx`s Strings bleiben hartcodiertes Deutsch statt
  `t()`-Übersetzungsschlüssel** — entspricht der bereits bestehenden
  Konvention dieser Datei (Spec 0020/0053, vor `t()`-Nutzung wie in
  `AboutSettings`/`FileTypeSettings`), keine Regression durch dieses Paket,
  aber auch keine Migration im Rahmen dieses Pakets.

## Konsequenzen

- Ein künftiger Audit-Log kann ohne Refactoring an den bestehenden
  `sftp_*`-Befehlen ansetzen (s. Punkt 4) — ein Nachweis dafür wäre erst
  beim tatsächlichen Bau dieses Features fällig.
- Drag-and-Drop-Verschieben zwischen Dateibrowser-Zeilen ist NICHT
  implementiert (nur Cut/Paste) — bewusste, von der Spec ausdrücklich
  erlaubte Vereinfachung (Punkt 2), kein offener Punkt.
- Sollte sich Polling (Punkt 3) als spürbar träge erweisen, ist ein
  Wechsel auf einen nativen Watcher lokal auf `useLocalEditSession.ts`
  beschränkt — keine API-Änderung für `FileBrowserPanel.tsx` nötig.
