# ADR 0106 — Single-Instance-Schutz und Sperre auf das Datenverzeichnis

Status: akzeptiert; §5/§6 und eine Konsequenz ergänzt durch ADR 0121
Betrifft: Issue #19, Spec 0101 §1, ADR 0093 §7, ADR 0032

## Problem

Zwei gleichzeitig laufende Prozesse konnten dieselbe Datenbank öffnen,
migrieren und sogar beide die Umwandlung der Klartext-Datei (Spec 0101, A6)
beginnen. Dazwischen stand nur SQLites eigene Dateisperre und die Prüfung
aus A6 Schritt 3. Issue #19 verlangt zwei unabhängige Schutzschichten:
einen Single-Instance-Schutz (zweiter Start holt die laufende Instanz nach
vorn) und eine exklusive Sperre auf das Datenverzeichnis, die vor jedem
Datenbankzugriff genommen und bis zum Prozessende gehalten wird. Einige
Punkte ließ das Issue offen.

## Entscheidung

1. **Sperre mit `std::fs::File::try_lock`, keine neue Abhängigkeit.** Die
   Standardbibliothek sperrt mit `flock` (macOS/Linux) bzw. `LockFileEx`
   (Windows). Das Betriebssystem gibt die Sperre beim Prozessende frei,
   auch nach einem Absturz oder `kill -9`. Die Datei `smart-ssh.lock`
   liegt neben `smart-ssh.db`, ihr Inhalt ist bedeutungslos, und sie wird
   nie gelöscht. Eine liegengebliebene Datei blockiert also nichts, nur ein
   laufender Halter tut das. Eine PID-Datei hätte genau das Problem der
   veralteten Einträge, das das Issue ausschließt.
2. **Die Sperre ist der erste Schritt in `app_shell::run`**, direkt nach dem
   Logging und vor `key_mode` (Verpackungsdatei), dem Schlüsselbund und der
   Datenbank. Sie lebt in `StartupInputs` und damit bis zum Prozessende,
   im Passwort-Modus über `PendingStartup` bis über die Entsperrung hinaus.
   Jeder Fehler beim Sperren beendet den Start (`lock_data_directory`), auch
   ein I/O-Fehler. Ein Start ohne Sperre findet nie statt.
3. **Die Umwandlung (A6) verlangt die Sperre als Argument.**
   `convert_plaintext_database(db_path, key, &DataDirLock)` lässt sich ohne
   gehaltene Sperre nicht aufrufen. Deckt die Sperre das Verzeichnis von
   `db_path` nicht (kanonisch verglichen, im Zweifel „nicht gedeckt"),
   endet sie mit `ConversionFailure::DataDirectoryNotLocked`, bevor eine
   Datei geöffnet ist. Dasselbe Argument trägt `open_or_prepare_database`.
   Die Prüfung aus Schritt 3 bleibt unverändert.
4. **Eigener Startfehler `ConnectFailureKind::AlreadyRunning`**, nicht
   `Other`: Der `Other`-Text rät unter anderem zu einem Backup. Hier ist
   nichts beschädigt. Der Text nennt die Ursache („Smart SSH is already
   running with this data directory") und als nächsten Schritt, das offene
   Fenster zu benutzen oder die andere Instanz zu beenden.
5. **Reihenfolge der beiden Schutzschichten.** Der Single-Instance-Schutz
   (`tauri-plugin-single-instance`) prüft in seinem Plugin-`setup`, also
   erst in `tauri::Builder::build`. Im Schlüsselbund-Modus ist die Datenbank
   zu diesem Zeitpunkt schon offen. Deshalb gilt:
   - **Sperre frei:** normaler Start, das Plugin ist das erste am Builder
     und meldet sich als laufende Instanz an.
   - **Sperre belegt:** Es wird eine App **nur mit dem Plugin** gebaut, ohne
     Ereignisschleife und damit ohne Fenster. Antwortet eine Instanz
     desselben Builds, kommt sie nach vorn, und der zweite Prozess endet
     mit 0. Antwortet keine (etwa ein anderer Build mit demselben
     Datenverzeichnis), nimmt der Prozess seine eigene Anmeldung sofort
     zurück (`destroy`) und zeigt den Startfehler. Ohne das Zurücknehmen
     würde eine danach startende echte Instanz den Prozess mit dem
     Fehlerdialog nach vorn holen und sich selbst beenden.
6. **Das Plugin ist nur in Release-Builds aktiv** (und seit ADR 0121 nur
   mit dem Standard-Datenverzeichnis). Es erkennt Instanzen an
   der App-Kennung (`identifier`), nicht am Datenverzeichnis. Debug- und
   Release-Build tragen dieselbe Kennung, haben aber nach ADR 0032 getrennte
   Datenverzeichnisse, damit beide nebeneinander laufen. Mit dem Plugin im
   Debug-Build würde ein `cargo tauri dev` neben einer installierten App
   sofort enden. Die Sperre auf das Datenverzeichnis gilt in jedem Build.
   Der Code ist in beiden Builds kompiliert (`cfg!` statt `#[cfg]`), damit
   Clippy und Build ihn auch im Debug-Gate sehen.

## Konsequenzen

- Zwei verschiedene Builds mit demselben Datenverzeichnis schließen sich
  gegenseitig aus. Der zweite zeigt den Startfehler, die Datenbank bleibt
  unberührt.
- Ein zweiter Start derselben Release-Version holt das offene Fenster nach
  vorn, statt einen Dialog zu zeigen.
- ~~Läuft dieselbe Release-Version mit einem anderen Datenverzeichnis, holt
  ein zweiter Start ebenfalls die erste Instanz nach vorn.~~ **Abgelöst
  durch ADR 0121 (Issue #44):** Das Plugin ist nur noch in Prozessen mit dem
  Standard-Datenverzeichnis aktiv. Release-Instanzen mit verschiedenen
  Datenverzeichnissen laufen nebeneinander, und kein zweiter Prozess öffnet
  eine Datenbank, bevor er wegen des Plugins endet. Der frühere Hinweis,
  eine der Instanzen als Debug-Build zu starten, entfällt.
- Auf Linux ohne D-Bus-Sitzung meldet das Plugin sich nicht an und startet
  normal. Den Schutz der Daten übernimmt auch dort die Sperre.
- Windows gibt die Sperre eines beendeten Prozesses laut Dokumentation von
  `LockFileEx` nicht zwingend sofort frei. Der Test für „nach `kill` startet
  ein neuer Prozess" versucht es deshalb bis zu 10 Sekunden lang. In der
  App zeigt ein so verspäteter Start den Startfehler, und ein erneuter
  Start gelingt.
