# 0020-manual-sftp-browser-design

## Status
Vorgeschlagen

## Kontext

`docs/specs/0020-sftp-file-transfer.md`, Abschnitt 5, gibt für den manuellen
Dateibrowser die Tauri-Command-Signaturen und eine grobe UI-Beschreibung vor
("Pfadleiste mit Navigation, Dateiliste, Kontextmenü, Upload per Button oder
Drag-and-Drop, Fortschrittsanzeige"), lässt aber mehrere konkrete Punkte
offen, die beim Umsetzen entschieden werden mussten.

## Entscheidungen

**1. Keine echte Byte-Fortschrittsanzeige — nur Start/Ende plus bekannte
Gesamtgröße.** Der `SftpSession`-Trait (Spec 0020, Abschnitt 3, bereits in
Teil 1 exakt wie spezifiziert committet) arbeitet mit `read_file`/
`write_file` auf einem einzelnen In-Memory-Puffer, nicht gestreamt — es gibt
keine Zwischenwerte, aus denen sich ein echter Prozentfortschritt ableiten
ließe. Statt eine Fortschrittsanzeige vorzutäuschen, die die Schnittstelle
nicht hergibt, sendet das Backend ein `sftp-transfer-started`/
`sftp-transfer-finished`-Ereignispaar (`crates/app-tauri/src/events.rs`),
mit der vorab per `stat()` (Download) bzw. lokaler Dateigröße (Upload)
ermittelten Gesamtgröße, falls verfügbar. Das Frontend zeigt dazwischen
einen pulsierenden, unbestimmten Fortschritt mit Dateiname und Größe.

**2. "Herunterladen"/"Löschen" nur für Dateien angeboten, "Umbenennen" für
beide.** Der Trait bietet nur `remove()` (SFTP `REMOVE`, wirkt ausschließlich
auf Dateien) — kein rekursives Verzeichnis-Löschen, kein
Mehrdatei-Download. Ein Löschversuch auf ein Verzeichnis würde serverseitig
mit einem für Nutzer:innen wenig aussagekräftigen Protokollfehler
scheitern. Das Kontextmenü blendet beide Aktionen deshalb für Verzeichnisse
von vornherein aus; Navigation (Doppelklick/Öffnen) und Umbenennen (SFTP
`RENAME` funktioniert für beide Eintragstypen) bleiben verfügbar.

**3. `local_path` bei `sftp_upload` kommt bereits vom Frontend, `sftp_download`
öffnet den Dialog selbst im Backend.** Bewusst asymmetrisch, aber beide
Varianten erfüllen "nie ohne expliziten Dialog auf die lokale Festplatte
zugreifen" (Spec 0020, Abschnitt 5): Beim Download steht das Ziel vorher
nicht fest, der native Speichern-Dialog muss also den Pfad liefern (wie bei
`export_document`, Spec 0012). Beim Upload steht die Quelle bereits fest,
bevor der Command aufgerufen wird — entweder über den nativen Öffnen-Dialog
(`@tauri-apps/plugin-dialog`, bereits etabliertes Muster aus
`frontend/src/fileDialog.ts`) oder über einen OS-Drag-and-Drop-Vorgang, bei
dem der Pfad direkt aus dem Drop-Ereignis kommt. Beides sind explizite
Nutzeraktionen, nur eben keine, die der *Backend*-Command selbst auslösen
müsste.

**4. Drag-and-Drop-Uploads sind auf den aktiven Tab beschränkt.** Jede
offene Session bleibt beim Tab-Wechsel gemountet (Spec 0017, Abschnitt 4) —
das gilt jetzt auch für `FileBrowserPanel`. Tauris
`getCurrentWebview().onDragDropEvent()` ist global für das gesamte Fenster,
nicht auf ein einzelnes DOM-Element scopebar. Ohne ein zusätzliches Gate
würden alle gleichzeitig gemounteten (aber unsichtbaren) Dateibrowser
anderer Tabs denselben Drop ebenfalls als Upload in ihr jeweiliges
Verzeichnis interpretieren. `SessionView` reicht deshalb ein `isActiveTab`-
Flag durch (`sessionTab.sessionId === activeSessionId` aus `App.tsx`), und
`FileBrowserPanel` abonniert Drag-and-Drop-Events nur, wenn sowohl der Tab
aktiv ist als auch die "Dateien"-Ansicht gerade gewählt ist.

**5. Terminal- und Dateibrowser-Ansicht bleiben beide gemountet, nur per
CSS umgeschaltet.** Analog zum bereits etablierten Muster für Session-Tabs
selbst (Spec 0017, Abschnitt 4): ein Unmount beim Umschalten würde sowohl
das xterm-Scrollback als auch die aktuelle Verzeichnisnavigation des
Dateibrowsers verwerfen.

## Konsequenzen

**Positiv:**
- Keine vorgetäuschten Fortschrittswerte — die UI zeigt genau das, was die
  Schnittstelle tatsächlich hergibt.
- Kein Nutzer-Frust durch einen scheiternden Löschversuch auf ein
  Verzeichnis — die Option existiert für diesen Fall gar nicht erst.
- Kein Cross-Tab-Datenleck bei Drag-and-Drop trotz des
  Alle-Tabs-bleiben-gemountet-Musters.

**Negativ / Trade-off:**
- Verzeichnisse lassen sich über den Dateibrowser aktuell nicht löschen —
  dafür bräuchte es entweder eine serverseitige Rekursion (im Trait nicht
  vorgesehen) oder eine clientseitige Rekursion über mehrere `list_dir`/
  `remove`-Aufrufe, die hier bewusst nicht gebaut wurde (Spec 0020 fordert
  das nicht explizit, und ein Fehlschlag mitten in einer rekursiven Löschung
  wäre ein neues Risiko für sich).
- Die Fortschrittsanzeige ist bei sehr großen Dateien weniger informativ als
  eine echte Prozentanzeige (kein ETA, kein Byte-Zähler) — eine spätere
  Erweiterung von `SftpSession` um eine Chunk-/Streaming-Schnittstelle
  könnte das nachrüsten, würde aber den bereits committeten Trait aus Teil 1
  rückwirkend ändern.
