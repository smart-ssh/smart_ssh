# Spec 0099 — Drittlizenzen im Release-Build und im Über-Bereich

Status: freigegeben · Backlog: BL-0054 · Gate: release-1.0/F
Zweck: Jeder Release-Build enthält die Lizenztexte aller ausgelieferten Rust- und npm-Abhängigkeiten und der mitgelieferten Schriften, und die App zeigt sie offline im Über-Bereich der Einstellungen an.
Review-Priorität: NORMAL

## 1. Ist-Stand (Stand `1526ea7`)

**Keine Drittlizenzen.** `NOTICE` im Wurzelverzeichnis hat zwei Zeilen
(Produktname, Copyright). Es gibt keine `about.toml`, keine Vorlage und
keine erzeugte Lizenzdatei (`git ls-files | grep -iE
'(^|/)(NOTICE|THIRD|LICENSE|about\.|deny\.toml|COPYING)'` findet nur
`LICENSE`, `NOTICE`, `deny.toml`).

**Erlaubte Lizenzen stehen in `deny.toml`** (`[licenses] allow`, elf
Einträge, darunter `MPL-2.0`). `cargo deny check licenses` läuft blockierend
im Job `dependency-audit` von `.github/workflows/community.yml`; die Werkzeuge
installiert dort `taiki-e/install-action` (`tool: cargo-deny,cargo-audit`)
(gelesen, nicht ausgeführt).

**Über-Bereich.** `AboutSettings`
(`apps/smart-ssh-community/frontend/src/components/AboutSettings.tsx`,
Kategorie der Einstellungen) zeigt Version, Edition und Build-Typ aus
`get_app_info` (`crates/app-shell/src/commands/app_meta.rs`), dazu einen
Kopieren-Knopf. Lizenzen zeigt er nicht. Locale-Schlüssel unter `about.*`,
Sprachen `de` und `en`, Gleichstand prüft `localeKeyParity.test.ts`
(gelesen, nicht ausgeführt).

**Release-Bau.** `.github/workflows/release.yml` baut per Matrix auf
macOS, Ubuntu 22.04 und Windows mit `tauri-apps/tauri-action`; davor nur
Checkout, Toolchains, Cache, Linux-Pakete, `npm ci`. Kein Lizenz-Schritt.
`tauri.conf.json` setzt kein `bundle.resources`; `beforeBuildCommand` ist
`npm run build` im Frontend. Der Job `test` der Community-CI baut App und
Frontend auf allen drei Plattformen (gelesen, nicht ausgeführt).

**CSP** (`tauri.conf.json`): `default-src 'self'`, `connect-src 'self' ipc:`.

**Schriften im Bundle.** Sieben `woff2`-Dateien unter
`apps/smart-ssh-community/frontend/src/assets/fonts/` (Barlow,
Barlow Condensed, JetBrains Mono), eingebunden per `@font-face` in
`src/index.css`. Daneben liegt keine Lizenzdatei.

**Gemessen** (Protokoll in der Beilage):
- `cargo-about` 0.9.2, `cargo about generate --format json --all-features`
  mit der `allow`-Liste aus `deny.toml` als `accepted`: Rückgabewert 0,
  33 s, keine Fehler. 705 Pakete (696 fremde, 9 aus dem eigenen Workspace),
  296 Lizenztexte; Übersicht laut `cargo-about` (Summe 739, Zählweise nicht
  geprüft): MIT 657, Apache-2.0 26,
  ISC 21, Unicode-3.0 19, BSD-3-Clause 8, MPL-2.0 5, CDLA-Permissive-2.0 2,
  Zlib 1. Der Lauf auf macOS enthält auch `windows-sys`, `webview2-com`
  und `gtk`.
- `cargo metadata`: 739 fremde Pakete. Die 43 nicht von `cargo-about`
  erfassten stehen im Lockfile, werden aber nicht gebaut (z. B.
  `sqlx-mysql`, `sqlx-postgres`, `quinn`, `tray-icon`). Eine Hinweisdatei
  hat nur `cfg_aliases` (`NOTICES.md`, zwei Versionen).
- npm, `npm ls --omit=dev --all`, dedupliziert nach (Name, Version):
  107 Pakete, alle MIT, ISC, Apache-2.0 oder deren `OR`-Kombination, jedes
  mit Lizenzdatei, keines mit `NOTICE`.
- **Ausgelieferter Code aus devDependencies:** Das gebaute CSS beginnt mit
  `/*! tailwindcss v4.3.3 | MIT License | https://tailwindcss.com */`; das
  gebaute JS enthält den `modulepreload`-Helfer von Vite. `tailwindcss`
  und `vite` stehen unter `devDependencies`.
- **Schriften** (Namenstabelle aller sieben Dateien, `fontTools`): Lizenz-URL
  `scripts.sil.org/OFL`; Copyright „Copyright 2017 The Barlow Project
  Authors (https://github.com/jpt/barlow)“ für alle Barlow-Dateien,
  „Copyright 2020 The JetBrains Mono Project Authors
  (https://github.com/JetBrains/JetBrainsMono)“ für JetBrains Mono. Der
  SPDX-Text `OFL-1.1` liegt offline im lokalen Cargo-Cache unter
  `~/.cargo/registry/src/*/spdx-0.13.4/src/text/licenses/OFL-1.1` (nicht
  Teil des Workspace; kam mit den Lizenzwerkzeugen).

## 2. Teil 0

Teil 0: entfällt — Werkzeug, Abhängigkeitsbaum, Bundle-Inhalt und
Schriftlizenzen sind gemessen; offen ist nur der Weg ins Paket, den T12
nachweist.

## 3. Ziel und Nicht-Ziele

Ziel: Lizenztexte entstehen beim Bau aus dem tatsächlichen
Abhängigkeitsbaum, landen im Release-Artefakt und sind in der App lesbar.

Nicht-Ziele:
- Die erzeugte Datei wird **nicht** eingecheckt.
- `NOTICE` und `LICENSE` des Projekts bleiben unverändert.
- Keine Änderung an `deny.toml` und an der Lizenzprüfung selbst.
- Andere Build-Wege, die diese App bauen, sind nicht Teil dieser Spec; sie
  müssen das Skript aus A1 aufrufen können (A1.4).
- Icons und Bilder des Projekts selbst.

## 4. Anforderungen

**A1 Erzeugung**
- A1.1 MUSS: Ein Skript im Repo erzeugt **eine** Textdatei mit den
  Lizenzen (a) aller Rust-Abhängigkeiten des Workspace, (b) aller
  Produktionsabhängigkeiten des Frontends, (c) der devDependencies, deren
  Code ins Bundle gelangt — heute mindestens `tailwindcss` und `vite` —,
  (d) der Schriften (A6). Je Lizenztext stehen die Pakete (Name, Version),
  die ihn verwenden; Pakete des eigenen Workspace dürfen fehlen.
- A1.2 MUSS: Hat ein Paket eine Hinweisdatei (`NOTICE*`, Groß/Klein egal),
  steht ihr Inhalt mit Paketname in der Ausgabe.
- A1.3 MUSS: Das Skript löscht zu Beginn eine vorhandene Ausgabe. Es bricht
  mit Rückgabewert ≠ 0 ab und hinterlässt keine Ausgabedatei, wenn ein
  Paket (Rust **oder** npm) eine nicht erlaubte Lizenz hat, wenn für ein
  Paket kein Lizenztext gefunden wird, oder wenn ein Werkzeug fehlt.
- A1.4 MUSS: Ausgabepfad, Workspace, Frontend und die Datei mit den
  erlaubten Lizenzen sind Parameter mit den heutigen Pfaden als Vorgabe.
- A1.5 MUSS: Die erlaubten Lizenzen haben **eine** Quelle. Entweder liest
  das Skript sie aus `deny.toml` (dann gilt sie auch für die npm-Seite),
  oder es gibt eine zweite Liste und ein Test in der CI scheitert, sobald
  beide voneinander abweichen.
- A1.6 MUSS: Die Werkzeugversion von `cargo-about` ist fest (Version
  angeben, nicht „latest“), lokal und in der CI dieselbe.
- A1.7 MUSS: Die Ausgabe ist gitignored.
- A1.8 MUSS: Die erzeugte Datei beginnt mit einer festen Kennzeile, an der
  die App sie erkennt (A4.3).

**A2 Release**
- A2.1 MUSS: Jeder Release-Build (alle drei Plattformen der Matrix) erzeugt
  die Datei vor dem App-Bau oder erhält sie aus einem vorgelagerten Job.
- A2.2 MUSS: Schlägt die Erzeugung fehl oder fehlt die Kennzeile (A1.8),
  scheitert der Release-Job; es entsteht kein Artefakt ohne Lizenzen.
- A2.3 MUSS: Die Datei ist Teil des ausgelieferten App-Pakets und ohne
  Netzwerk lesbar.
- A2.4 MUSS: `npm run build` und `cargo build` gelingen weiterhin ohne die
  Datei (Entwicklung, Job `test` der Community-CI).

**A3 CI**
- A3.1 MUSS: Die Community-CI fährt das Skript bei jedem Lauf einmal
  (eine Plattform genügt) und wird rot, wenn es scheitert.

**A4 Anzeige**
- A4.1 MUSS: Der Über-Bereich hat einen Eintrag „Drittanbieter-Lizenzen“
  (EN „Third-party licenses“), der den Text in einer scrollbaren Ansicht
  innerhalb der App öffnet.
- A4.2 MUSS: Der Text wird als reiner Text dargestellt, nie als HTML oder
  Markdown mit eingebettetem HTML.
- A4.3 MUSS: Ob die Liste vorhanden ist, erkennt die App an der Kennzeile
  (A1.8), nicht nur an einem Ladefehler. Fehlt sie (Entwicklungs-Build,
  oder der Ladeweg liefert etwas anderes, etwa die Startseite der App),
  zeigt die Ansicht einen Hinweis, dass die Liste nur in Release-Builds
  enthalten ist, und kein leeres Feld, keinen fremden Inhalt und keinen
  Absturz.
- A4.4 MUSS: Das Laden der Liste löst keine Anfrage an einen fremden Host
  aus.
- A4.5 MUSS: Neue Texte in DE und EN.

**A5 Changelog** — MUSS: Fragment unter `changelog.d/`.

**A6 Schriften** — MUSS: Neben den Schriftdateien liegt eine Lizenzdatei
mit den beiden Copyright-Zeilen aus §1 und dem vollständigen Text der SIL
Open Font License 1.1 (SPDX-Text `OFL-1.1`, offline verfügbar, s. §1). Ihr
Inhalt steht in der erzeugten Ausgabe. Die Schriftlizenz wird **nicht**
gegen die erlaubte Liste geprüft (Lizenz fest OFL-1.1, Quelle ist diese
Datei); `deny.toml` bleibt unverändert.

## 5. Design

- Wie die Datei ins Paket kommt (statisches Frontend-Asset, Tauri-Ressource
  oder eingebettet in die Binärdatei) entscheidet der Coder. A2.3, A2.4,
  A4.3 und A4.4 müssen gelten.
- Ob die Release-Matrix selbst erzeugt oder einen vorgelagerten Job nutzt,
  entscheidet der Coder. `cargo-about` sammelt über alle Zielplattformen;
  die Ausgabe hängt nicht davon ab, auf welchem System sie entsteht.
- Die npm-Seite braucht kein neues Paket; Name, Version, Lizenzfeld und
  Lizenzdatei liegen in `node_modules/<paket>/`. Welche devDependencies
  Code ins Bundle bringen, prüft der Coder am gebauten `dist` (Banner,
  Helfer) und nennt sie im Bericht.

## 6. Sicherheits-Invarianten

- **Fremder Inhalt bleibt Text.** Lizenz- und Hinweisdateien stammen aus
  fremden Paketen. Sie erreichen die Webview nur als Text (A4.2), damit
  kein Paket über seine Lizenzdatei Markup oder Skript einschleust.
- **Kein Netzwerk ohne Nutzeraktion** (BL-0042): A4.4; zweite Linie ist die
  CSP (`connect-src 'self' ipc:`).
- **Lieferkette des Release-Laufs.** Neue Werkzeuge im Release-Workflow
  kommen mit fester Version (A1.6); keine neue Action ohne festen Stand.
- Filter, Redactor, Credentials und Ausführungspfad sind nicht berührt.

## 7. Tests

- **T1 Erzeugung auf dem heutigen Stand** (A1.1, A1.8): Skript läuft mit
  Rückgabewert 0; die Ausgabe beginnt mit der Kennzeile und enthält
  mindestens die Texte zu MIT, Apache-2.0, MPL-2.0 und OFL-1.1, die
  Copyright-Zeilen von Barlow und JetBrains Mono sowie die Pakete `tauri`,
  `react`, `tailwindcss` und `vite`. Scheitert, wenn eine
  der vier Quellen aus A1.1 fehlt.
- **T2 Hinweisdatei** (A1.2): Der Inhalt von `NOTICES.md` aus `cfg_aliases`
  steht in der Ausgabe. Scheitert, wenn Hinweisdateien übergangen werden.
- **T3 Abbruchgründe** (A1.3, Negativfälle; von Hand zulässig, je Befehl
  und Ausgabe in den Bericht). Vor jedem Fall liegt eine Ausgabe aus einem
  früheren Lauf bereit; danach muss sie fehlen und der Rückgabewert ≠ 0 sein:
  - (a) erlaubte Lizenzen ohne `MIT` (über den Parameter aus A1.4);
  - (b) ein npm-Paket mit verfälschtem Lizenzfeld in einer Kopie von
    `node_modules` — scheitert, wenn nur die Rust-Seite geprüft wird;
  - (c) ein npm-Paket ohne Lizenzdatei in einer Kopie von `node_modules`;
  - (d) `cargo-about` nicht im `PATH`.
- **T4 Eine Quelle** (A1.5): Liest das Skript `deny.toml`, führt eine aus
  der Datei entfernte, aber benutzte Lizenz zum Abbruch. Gibt es eine
  zweite Liste, macht ein Eintrag nur in einer der beiden die CI rot.
  Gegenbeweis mit verfälschter Kopie in den Bericht.
- **T5 npm vollständig** (A1.1): Der Test zählt zur Laufzeit per
  `npm ls --omit=dev --all --json`, dedupliziert nach (Name, Version),
  **unabhängig vom Code des Skripts**, nimmt die devDependencies aus
  A1.1(c) hinzu und verlangt **Gleichheit** mit den npm-Paketen der Ausgabe
  (heute 107 + 2 als Referenz, nicht fest eingetragen). Scheitert bei
  fehlenden wie bei überzähligen Paketen.
- **T6 Anzeige mit Datei** (A4.1): Oberflächentest — Klick auf den Eintrag
  öffnet die Ansicht mit dem Text.
- **T7 Anzeige ohne gültige Datei** (A4.3): (a) Laden scheitert, (b) Laden
  liefert HTML ohne Kennzeile → in beiden Fällen der Hinweis „nur in
  Release-Builds“, kein geladener Inhalt sichtbar, keine Ausnahme.
  Scheitert, wenn nur der Ladefehler geprüft wird.
- **T8 Markup bleibt Text** (A4.2, adversarial): Ein Lizenztext mit
  `<img src=x onerror=alert(1)>` und `<script>` erscheint wörtlich als
  Text, im DOM entsteht kein `img`- oder `script`-Element. Scheitert bei
  `dangerouslySetInnerHTML` oder einem Markdown-Renderer mit HTML.
- **T9 Kein fremder Host** (A4.4): Im Komponententest wird `fetch`, falls
  überhaupt, nur mit einer relativen Adresse aufgerufen.
- **T10 Release-Abbruch** (A2.2): Nicht in der CI ausführbar. Der Coder
  zeigt im Bericht den Workflow-Schritt, der abbricht, und fährt seine
  Prüfbedingung lokal einmal mit leerer Datei, einmal mit einer Datei ohne
  Kennzeile (beide müssen scheitern) und einmal mit der echten (muss
  durchgehen).
- **T11 Locale-Gleichstand**: bestehender `localeKeyParity.test.ts` grün.
- **T12 Im Paket** (A2.3, A2.4; Handprüfung, Pflichtbeleg im Bericht):
  Skript fahren, Release-Bundle lokal bauen, die Datei im Bundle bzw. im
  eingebetteten Asset nachweisen (Pfad oder Befehl mit Ausgabe), das
  gebaute Paket ohne Netz starten und die Liste im Über-Bereich öffnen.
  Danach Ausgabe löschen, `npm run build` und `cargo build` erneut: beide
  gelingen, die App zeigt den Hinweis aus A4.3.

## 8. Offene Punkte

Keine. Entschieden: Erzeugung beim Release-Bau statt eingecheckter Datei.

## 9. Klarstellungen

- **2026-10-01 · Release-Lauf v0.5.2, Windows rot** · Unter PowerShell
  bricht `cargo about generate` ab, wenn seine Ausgabe nicht per
  `-o`/`--output-file` in eine Datei geht („should not redirect its output
  in powershell“). Das Skript schreibt die Ausgabe von `cargo-about` deshalb
  in eine Datei und liest sie von dort. Ebenso startet es `npm` unter
  Windows so, dass `npm.cmd` gefunden wird. **A3.1 wird erweitert:** Die
  Community-CI fährt das Skript auf **allen drei** Plattformen ihrer Matrix,
  nicht nur auf einer, damit ein Plattformfehler vor dem Release-Lauf
  auffällt.

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `docs(frontend): add the font license next to the bundled fonts [BL-0054]` — A6 (geprüft über T1).
2. `build: generate third-party license notices from the dependency tree [BL-0054]` — A1, T1–T5.
3. `ci: generate the license notices in every release build and in CI [BL-0054]` — A2, A3, T10.
4. `feat(frontend): show third-party licenses in the about settings [BL-0054]` — A4, T6–T9, T11, T12.
5. `docs(changelog): note the third-party licenses in the about settings [BL-0054]` — A5.

**Priorität:** NORMAL. Prüfpunkte für den Review: Fehler beider Seiten
werden durchgereicht (T3); die Anzeige rendert nichts als HTML (T8) und
erkennt die Datei am Inhalt (T7); der Release-Job kann nicht ohne Datei
grün werden (T10); die Datei liegt wirklich im Paket (T12).

**Aufteilung:** ein Lauf auf Sonnet. Skript, Workflows und Oberfläche
hängen über den Dateipfad und die Kennzeile zusammen; kein Sicherheitskern.

**Berührte Module:** neues Skript und Konfiguration im Repo,
`.gitignore`, `.github/workflows/release.yml`,
`.github/workflows/community.yml`, Lizenzdatei unter
`frontend/src/assets/fonts/`, ggf. `tauri.conf.json` und
`apps/smart-ssh-community/build.rs`, Frontend `AboutSettings` und Locales,
`changelog.d/`.

**Melde zurück:** gewählter Weg ins Paket (A2.3) und warum; Größe der
Ausgabedatei; welche devDependencies Code ins Bundle bringen; Belege zu T3,
T4, T10 und T12; manueller Testablauf (Release-Build lokal, Über-Bereich
öffnen, Dev-Build zeigt den Hinweis).
