# ADR 0113 — Freigaben für lokale Pfade beim Upload

Status: akzeptiert
Betrifft: Spec 0020 (Abschnitt 5.2), Spec 0054 (Teil 3 und 4), Issue #89

## Problem

`sftp_upload`, `read_local_text_preview` und `local_file_mtime` nahmen
einen lokalen Pfad aus dem Webview entgegen und lasen ihn ohne Prüfung.
Das Webview rendert auch KI-erzeugte Inhalte und ist deshalb keine
Vertrauensgrenze. Code darin konnte die App jede lokale Datei lesen und auf
den Server schreiben lassen (z. B. `~/.ssh/id_ed25519`), oder ihren Inhalt
über die Überschreib-Vorschau zurückbekommen.

## Entscheidung

1. **Freigaben je Sitzung, im Backend.** `app_logic::local_path_grants::
   LocalPathGrants` hält je `SessionId` die freigegebenen Wurzeln. Eine
   Wurzel entsteht nur aus einer Nutzergeste, die das Webview nicht
   nachstellen kann:
   - **Öffnen-Dialog im Backend** (`pick_upload_files`, Muster
     `read_credential_file`): das Frontend gibt nur einen Titel mit.
   - **Natives Drag-and-Drop:** `Builder::on_window_event` nimmt
     `WindowEvent::DragDrop(Drop { paths })` entgegen und merkt sich die
     Pfadliste als „letztes Ablegen“. Der Dateibrowser, der das gleiche
     Ereignis im Webview sieht, ruft `claim_dropped_paths(session_id)`; das
     Backend gibt die gemerkten Pfade dieser Sitzung frei und liefert sie
     zurück. Das Webview nennt keine Pfade, nur die Sitzung.
   - **Eigene Bearbeitungskopie:** Dateien unter `edit_session_dir` der
     Sitzung zählen ohne Eintrag, wie bei `close_edit_session`.
2. **Vergleich auf kanonischen Pfaden.** Wurzeln werden beim Freigeben
   kanonisiert, der angefragte Pfad bei der Prüfung; geprüft wird
   komponentenweise (`Path::starts_with`). `..` und Symlinks, die aus einer
   Wurzel hinausführen, treffen damit keine Freigabe. Gelesen wird der
   kanonische Pfad, nicht der übergebene.
3. **Typ statt Konvention.** Die Leser nehmen ein `GrantedLocalPath`, das
   nur `LocalPathGrants::check` erzeugt. Ein ungeprüfter Pfad kommt so gar
   nicht erst bis zum Lesen. Ein Test-Konstruktor existiert nur hinter
   `test`/`test-support`.
4. **Lebensdauer.** `disconnect` entfernt die Freigaben der Sitzung.
   Zusätzlich verlangt jede Prüfung, dass die Sitzung noch existiert; so
   zählt weder eine Freigabe, die im Moment des Trennens noch eingetragen
   wurde, noch eine liegengebliebene Bearbeitungskopie. Sitzungs-IDs werden
   nicht wiederverwendet.
5. **Ablegen verfällt.** Ein nicht abgeholtes Ablegen gilt höchstens
   `DROP_CLAIM_WINDOW` (10 s) und nur einmal. Ein Ablegen, das kein
   sichtbarer Dateibrowser abholt (z. B. auf den Chat), liegt also nicht
   für eine spätere, fremde Abholung bereit. Innerhalb dieser 10 s ist das
   Abholen allerdings an keine Sitzung und kein Ablageziel gebunden, siehe
   „Restrisiko: Abholen ist nicht an Sitzung oder Ziel gebunden“.
6. **`local_file_mtime` meldet bei Ablehnung `None`,** wie für eine fehlende
   Datei. Für eine Hintergrundabfrage ist beides „kein verlässlicher
   Zeitstempel“, und so verrät der Befehl dem Webview nicht, welche
   beliebigen Pfade existieren. Upload und Vorschau scheitern dagegen mit
   einer Meldung.

### Reihenfolge Ablegen → Abholen

Tauri stellt das Drop-Ereignis auf dem Event-Loop-Thread zu: erst wird es
an das Webview gesendet, dann laufen im selben Aufruf die
`on_window_event`-Handler. Das Webview kann frühestens danach reagieren,
und sein `invoke` wird auf demselben Thread angenommen. Das gemerkte
Ablegen steht also bereit, bevor `claim_dropped_paths` ankommt; ein Warten
im Befehl ist nicht nötig.

### Restrisiko: Abholen ist nicht an Sitzung oder Ziel gebunden

Das gemerkte Ablegen ist **global**: `record_drop` speichert genau eine
Pfadliste für die ganze App, ohne Sitzung, Fenster oder Ablageposition.
`claim_dropped_paths` prüft nur, dass die nennende Sitzung existiert;
danach gibt `claim_drop` das Ablegen dieser Sitzung frei, wenn es höchstens
`DROP_CLAIM_WINDOW` (10 s) alt ist. Ob es auf dem Dateibrowser dieser
Sitzung, auf dem einer anderen oder überhaupt auf einem Dateibrowser
gelandet ist, prüft das Backend nicht.

- **Jede lebende Sitzung kann ein Ablegen abholen,** innerhalb von
  `DROP_CLAIM_WINDOW` und genau einmal. Das erste Abholen verbraucht es;
  jede weitere Sitzung bekommt danach eine leere Liste.
- **„Nur der sichtbare Dateibrowser holt ab“ ist eine Regel des
  Frontends,** nicht des Backends. Code im Webview, das keine
  Vertrauensgrenze ist (siehe „Problem“), könnte ein Ablegen auf den Chat
  oder eines, das für eine andere Sitzung gedacht war, innerhalb der 10 s
  für eine beliebige lebende Sitzung abholen.

**Warum das Risiko getragen wird:** Freigegeben werden können nur Pfade,
die der Nutzer wirklich auf das Fenster gezogen hat — beliebige Pfade
lassen sich so nicht lesen. Das Fenster ist kurz, und ein Abholen
verbraucht das Ablegen, sodass derselbe Drop nicht mehreren Sitzungen
zufallen kann. Der Schaden beschränkt sich darauf, dass eine abgelegte
Datei auf einem anderen der verbundenen Server landen kann als gemeint.

**Mögliche Verengung, falls nötig:** Das Backend merkt sich beim Ablegen
die Ablageposition oder die fokussierte Sitzung und prüft beim Abholen,
dass die nennende Sitzung dazu passt; ein Abholen durch eine andere
Sitzung wird abgelehnt. Dafür müsste das Backend eine Fensterposition
einem Dateibrowser zuordnen können, was es heute nicht kann. Das wäre eine
eigene Designänderung.

Der Test `test_a_drop_is_claimable_by_any_session_but_only_once` in
`local_path_grants` hält dieses Verhalten fest: Sitzung B holt ein Ablegen
ab, das für Sitzung A gedacht war, und A bekommt danach eine leere Liste.
Ändert sich das Verhalten (z. B. durch die Verengung oben), schlägt der
Test fehl, und dieser Abschnitt ist mit anzupassen.

## Bewusst nicht Teil dieser Änderung: Ordner-Upload

Das Issue nahm an, dass Ordner-Uploads bereits funktionieren. Das stimmt
nicht: Der Upload liest genau eine Datei, ein abgelegter Ordner scheitert
beim Lesen, und der Upload-Dialog erlaubt keine Ordnerwahl. Entschieden
wurde, den Ordner-Upload hier **nicht** einzuführen — er wäre ein neues
Feature mit eigenen Fragen (Konflikte im Ordner, Symlinks darunter,
teilweises Scheitern, Fortschritt) in einer Härtungsänderung.

Das Freigabemodell deckt Ordner trotzdem schon ab: Ein abgelegter Ordner
wird als Wurzel freigegeben, Dateien darunter gelten als freigegeben. Ein
späterer rekursiver Upload braucht deshalb keine Änderung an dieser
Grenze. Ein abgelegter Ordner verhält sich bis dahin wie zuvor: Er besteht
die Freigabeprüfung und scheitert beim Lesen mit der bisherigen Meldung.

## Abgewogene Alternativen

- **Pfade aus dem Webview-Ereignis übernehmen und nur gegen eine
  Positivliste prüfen:** Das Webview-Ereignis lässt sich aus dem Webview
  heraus nachstellen; nur das native Fenster-Ereignis belegt eine
  Nutzergeste.
- **Den Inhalt beim Ablegen sofort lesen:** Große Dateien müssten ohne
  Zielverzeichnis und vor der Überschreib-Rückfrage im Speicher liegen.
- **Freigaben global statt je Sitzung:** Eine Datei, die für Server A
  abgelegt wurde, könnte sonst auf Server B hochgeladen werden.
- **Pfad-Präfixvergleich auf Zeichenketten:** `/a/project` würde
  `/a/project-secrets` einschließen; deshalb komponentenweise.
