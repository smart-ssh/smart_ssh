# ADR 0114 — Bearbeitungskopien je Datenverzeichnis, Aufräumen beim Start

Status: akzeptiert
Betrifft: Spec 0054 (Teil 4, Punkt 6), ADR 0032, Issue #19, Issue #93

## Problem

„Lokal öffnen“ legt die Remote-Datei im Klartext unter
`<cache>/smart-ssh/edit-sessions/<session_id>/` ab. Aufgeräumt wurde nur
beim Beenden der Bearbeitung und beim Trennen der Sitzung. Nach einem
Absturz passt keine spätere `session_id` mehr zum alten Verzeichnis, die
Kopie blieb also für immer liegen. Debug- und Release-Build laufen mit
getrennten Datenverzeichnissen (ADR 0032) nebeneinander, teilen aber das
Cache-Verzeichnis. Ein Aufräumen des ganzen Ordners hätte die offenen
Kopien der jeweils anderen Instanz gelöscht.

## Entscheidung

1. **Ordner je Datenverzeichnis.** Neue Lage:
   `<cache>/smart-ssh/edit-sessions/data-<fnv1a64(datenverzeichnis)>/<session_id>/`.
   Grundlage ist der kanonische Pfad, den die Datenverzeichnis-Sperre
   (Issue #19) hält. FNV-1a ist von Hand geschrieben statt `DefaultHasher`,
   dessen Algorithmus sich zwischen Rust-Versionen ändern darf — ein
   anderer Wert nach einem Update ließe den alten Ordner verwaist zurück.
   Keine neue Abhängigkeit.
2. **Aufräumen direkt nach dem Sperren.** `run()` ruft das Aufräumen
   unmittelbar nach `lock_data_directory` auf, vor dem Aufbau des
   Zustands und vor dem Fenster. Mit gehaltener Sperre nutzt kein anderer
   Prozess dieses Datenverzeichnis, und es existiert noch keine Sitzung;
   jeder Eintrag im eigenen Ordner ist ein Überbleibsel. Im Passwort-Modus
   (Spec 0101) gilt dasselbe, das Aufräumen braucht keine Datenbank.
3. **Best effort, ohne Inhalt.** Jeder Fehler wird mit Pfad und Fehler per
   `tracing::warn!` protokolliert, das Aufräumen fährt mit dem nächsten
   Eintrag fort, der Start läuft weiter.
4. **Links nie folgen.** Einträge werden per `symlink_metadata` beurteilt:
   ein Link wird als Link entfernt (unter Windows ein Verzeichnis-Link per
   `remove_dir`), ein echtes Verzeichnis per `remove_dir_all`, das Links
   darin ebenfalls nicht folgt. Ein Link anstelle des eigenen Ordners wird
   selbst entfernt, sein Ziel nicht durchsucht.
5. **Alte gemeinsame Ablage bleibt unberührt.** Sitzungsordner früherer
   Versionen liegen direkt in `edit-sessions/`, außerhalb jedes
   `data-*`-Ordners. Das Aufräumen fasst sie nie an. Die Sperre beweist nur,
   dass kein anderer Prozess *dieses* Datenverzeichnis nutzt. Eine ältere
   Version mit *anderem* Datenverzeichnis (ein Debug-Build oder ein über
   `SMART_SSH_DATA_DIR` umgelenkter Build) sieht sie nicht. Diese Version
   kann gerade eine Datei lokal geöffnet haben, und ihre Kopie liegt in der
   alten Ablage. Ein Ordner der alten Ablage verrät nicht, zu welcher
   Instanz er gehört. Es gibt also keinen Weg zu beweisen, dass ihn niemand
   mehr nutzt, und eine Heuristik (Alter, Build-Art) wäre kein Beweis.
   Verworfen wurde deshalb, dass ein Release-Build ohne Umlenkung die alte
   Ablage räumt: Er hätte in diesem Fall die offene Kopie einer anderen,
   laufenden Instanz gelöscht.

## Folgen

- Normales Aufräumen beim Beenden der Bearbeitung und beim Trennen bleibt
  unverändert; es arbeitet auf dem neuen Ordner.
- Die Rechte neuer Kopien bleiben `0700`/`0600` (Unix); das Anlegen ist nur
  in eine eigene Funktion gewandert.
- Kopien, die eine frühere Version nach einem Absturz liegen gelassen hat,
  bleiben in `<cache>/smart-ssh/edit-sessions/` direkt liegen, bis der
  Nutzer sie selbst löscht. Neue Überbleibsel entstehen dort nicht mehr. Der
  Changelog-Eintrag weist darauf hin.
