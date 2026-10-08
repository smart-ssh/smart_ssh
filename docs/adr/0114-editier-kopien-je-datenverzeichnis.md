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
5. **Alte gemeinsame Ablage.** Sitzungsordner früherer Versionen liegen
   direkt in `edit-sessions/` und beginnen nie mit `data-`. Sie entfernt
   nur ein **Release-Build ohne umgelenktes Datenverzeichnis**
   (`SMART_SSH_DATA_DIR`); die `data-*`-Ordner aller Instanzen bleiben
   dabei stehen. Eine ältere Release-Version kann nicht daneben laufen
   (gleiches Datenverzeichnis, gleiche Sperre). Nicht sehen kann die
   Sperre eine ältere Version mit *anderem* Datenverzeichnis, also einen
   Debug- oder umgelenkten Build — eine Entwicklungsumgebung. Dieser Rest
   ist bewusst in Kauf genommen: Ein solcher Build verlöre im ungünstigsten
   Fall seine lokale Kopie, nie die Datei auf dem Server, und der Editor
   behält seinen Inhalt. Debug- und umgelenkte Builds räumen die alte
   Ablage nie auf, damit sich zwei Entwicklungs-Builds nicht gegenseitig
   etwas löschen.

## Folgen

- Normales Aufräumen beim Beenden der Bearbeitung und beim Trennen bleibt
  unverändert; es arbeitet auf dem neuen Ordner.
- Die Rechte neuer Kopien bleiben `0700`/`0600` (Unix); das Anlegen ist nur
  in eine eigene Funktion gewandert.
- Auf einer Entwicklungsmaschine bleibt die alte Ablage bestehen, bis ein
  Release-Build ohne Umlenkung startet.
