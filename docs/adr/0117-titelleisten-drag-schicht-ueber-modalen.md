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
   Einstellungsdialog behält seinen Backdrop ohne z-Index) und meldet ihr
   Backdrop-Element beim Mounten in einem Modul-Speicher (`modalLayer.ts`)
   an, beim Unmounten wieder ab. Ein Modul-Speicher statt eines React-Contexts, weil Dialoge
   teils per Portal nach `document.body` gerendert und teils über die
   Erweiterungs-Registry beigesteuert werden, also nicht zuverlässig unter
   einem gemeinsamen Provider liegen. `ModalBackdrop` wird auch über
   `extensions/index.ts` angeboten.
2. **Nur sichtbare Backdrops zählen.** Inaktive Session-Tabs, die
   Startansicht und Teilbereiche einer Session bleiben gemountet und werden
   per `display:none` ausgeblendet. Ein dort inline (ohne Portal)
   gerenderter Dialog bleibt nach einem Tab-Wechsel per Tastatur
   angemeldet, ist aber unsichtbar. `modalLayer` zählt daher nur einen
   angemeldeten Backdrop, bei dem weder er selbst noch ein Vorfahre
   `display:none` hat. Weil ein solcher Wechsel nur eine Klasse an einem
   Vorfahren ändert, beobachtet ein `MutationObserver` Änderungen an
   `class`, `style` und `hidden` im Dokument und prüft dann neu. Er läuft
   nur, solange mindestens ein Backdrop angemeldet ist. Verworfen wurde ein
   Sichtbarkeits-Context, den jeder ausblendende Container setzt. Jeder
   neue Container müsste daran denken, und ein per Portal gerenderter
   Dialog erbt den Context, obwohl er sichtbar bleibt.
3. **Drag-Schicht nur bei offenem Dialog.** Solange ein angemeldeter
   Backdrop sichtbar ist,
   rendert `AppHeader` direkt neben dem `<header>` eine leere, transparente
   `fixed`-Schicht in Header-Höhe mit `data-tauri-drag-region`. Ohne Dialog
   existiert sie nicht; Tabs und Drag-Region im Header bleiben dann
   unverändert. Bei offenem Dialog deckt sie die Header-Inhalte ab — Klicks
   auf Tabs erreichen sie nicht mehr.
4. **Doppelklick und Ziehen über Tauris Drag-Skript.** Tauris
   eingebautes Drag-Skript behandelt `data-tauri-drag-region` auf allen
   drei Plattformen: Ziehen beim Mausdruck, Doppelklick maximiert/zoomt
   (macOS beim Loslassen). Die Schicht braucht dafür keinen eigenen
   Handler; sie verhält sich wie der Header ohne Dialog.
5. **Stapelung: 50 < 60 < 100.** Backdrops `z-50`, Drag-Schicht 60,
   Fenster-Controls von `tauri-plugin-decoration` 100 (von der App per
   `--tauri-plugin-decoration-z-index` gesetzt). Die HTML-Controls unter
   Windows/Linux lagen damit schon vorher über den Backdrops; die Schicht
   lässt ihren Bereich zusätzlich frei (rechter bzw. linker Freiraum aus
   denselben CSS-Variablen wie das Header-Padding). Die macOS-Ampel ist
   nativ und liegt ohnehin über der Webview.
6. **Schicht als Geschwister, nicht als Kind des Headers.** Der Header
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
- Andere Arten des Ausblendens (`visibility: hidden`, Größe 0, außerhalb
  des Fensters) erkennt die Prüfung nicht. Die App blendet Ansichten
  ausschließlich per `display:none` aus.
- Eine Erweiterung außerhalb dieses Repos, die einen eigenen Backdrop
  ohne `ModalBackdrop` baut, bekommt das Verhalten nicht; der Guard-Test
  sieht nur dieses Repo.
