# ADR 0117 — Titelleisten-Drag-Schicht über modalen Dialogen

Status: akzeptiert
Betrifft: Spec 0014 (Abschnitt 5), Issue #160

## Kontext

Jeder modale Dialog legt einen Backdrop (`fixed inset-0`, meist `z-50`)
über das ganze Fenster, die eigene Titelleiste eingeschlossen. Mausklicks
auf die Titelleiste treffen dann den Backdrop statt der Drag-Region; das
Fenster ließ sich bei offenem Dialog nicht verschieben. Issue #160 verlangt
einen gemeinsamen Mechanismus für alle Dialoge (auch künftige und über
Erweiterungen beigesteuerte), ohne optische Änderung und ohne Änderung des
Verhaltens ohne Dialog. Offen ließ das Issue, wie die App erfährt, dass ein
Dialog offen ist, und wie die Schicht gestapelt wird.

## Entscheidung

1. **Gemeinsamer Backdrop mit Anmeldung.** Alle modalen Dialoge rendern
   ihren Backdrop über die Komponente `ModalBackdrop`. Sie übernimmt die
   Klassen des Aufrufers unverändert (kein optischer Unterschied, auch der
   Einstellungsdialog behält seinen Backdrop ohne z-Index) und meldet den
   Dialog beim Mounten in einem Modul-Zähler (`modalLayer.ts`) an, beim
   Unmounten wieder ab. Ein Zähler statt eines React-Contexts, weil Dialoge
   teils per Portal nach `document.body` gerendert und teils über die
   Erweiterungs-Registry beigesteuert werden, also nicht zuverlässig unter
   einem gemeinsamen Provider liegen. `ModalBackdrop` wird auch über
   `extensions/index.ts` angeboten.
2. **Drag-Schicht nur bei offenem Dialog.** Solange der Zähler > 0 ist,
   rendert `AppHeader` direkt neben dem `<header>` eine leere, transparente
   `fixed`-Schicht in Header-Höhe mit `data-tauri-drag-region`. Ohne Dialog
   existiert sie nicht; Tabs und Drag-Region im Header bleiben dann
   unverändert. Bei offenem Dialog deckt sie die Header-Inhalte ab — Klicks
   auf Tabs erreichen sie nicht mehr.
3. **Doppelklick und Ziehen über Tauris Drag-Skript.** Tauris
   eingebautes Drag-Skript behandelt `data-tauri-drag-region` auf allen
   drei Plattformen: Ziehen beim Mausdruck, Doppelklick maximiert/zoomt
   (macOS beim Loslassen). Die Schicht braucht dafür keinen eigenen
   Handler; sie verhält sich wie der Header ohne Dialog.
4. **Stapelung: 50 < 60 < 100.** Backdrops `z-50`, Drag-Schicht 60,
   Fenster-Controls von `tauri-plugin-decoration` 100 (von der App per
   `--tauri-plugin-decoration-z-index` gesetzt). Die HTML-Controls unter
   Windows/Linux lagen damit schon vorher über den Backdrops; die Schicht
   lässt ihren Bereich zusätzlich frei (rechter bzw. linker Freiraum aus
   denselben CSS-Variablen wie das Header-Padding). Die macOS-Ampel ist
   nativ und liegt ohnehin über der Webview.
5. **Schicht als Geschwister, nicht als Kind des Headers.** Der Header
   trägt `backdrop-blur`; das macht ihn zum Bezugsrahmen für
   `fixed`-Nachfahren und zu einem eigenen Stapelkontext. Eine Schicht im
   Header käme nicht über die Backdrops.

## Konsequenzen

- Ein neuer modaler Dialog muss `ModalBackdrop` nutzen. Ein Test prüft alle
  Nicht-Test-Quelldateien des Frontends auf Elemente mit `fixed` und
  `inset-0` außerhalb von `ModalBackdrop` und schlägt sonst fehl.
- Ein Dialog oder Overlay mit z-Index ≥ 60 würde die Titelleiste wieder
  verdecken; der Wert steht kommentiert in `modalLayer.ts` neben der
  Header-Komponente.
- Eine Erweiterung außerhalb dieses Repos, die einen eigenen Backdrop
  ohne `ModalBackdrop` baut, bekommt das Verhalten nicht; der Guard-Test
  sieht nur dieses Repo.
