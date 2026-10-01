# Spec 0099 — Drittlizenzen im Release-Build und im Über-Bereich

Status: Vorschlag (Architekt) · Backlog: BL-0054 · Gate: release-1.0/F
Zweck: Jeder Release-Build enthält die Lizenztexte aller ausgelieferten Rust- und npm-Abhängigkeiten, und die App zeigt sie offline im Über-Bereich der Einstellungen an.
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
installiert dort `taiki-e/install-action` (`tool: cargo-deny,cargo-audit`).

**Über-Bereich.** `AboutSettings` (Frontend, Kategorie der Einstellungen)
zeigt Version, Edition und Build-Typ aus `get_app_info`
(`crates/app-shell/src/commands/app_meta.rs`), dazu einen Kopieren-Knopf.
Lizenzen zeigt er nicht. Locale-Schlüssel unter `about.*`, Sprachen `de`
und `en`, Gleichstand der Schlüssel prüft `localeKeyParity.test.ts`.

**Release-Bau.** `.github/workflows/release.yml` baut per Matrix auf
macOS, Ubuntu 22.04 und Windows mit `tauri-apps/tauri-action`; davor nur
Checkout, Toolchains, Cache, Linux-Pakete, `npm ci`. Kein Lizenz-Schritt.
`tauri.conf.json` setzt kein `bundle.resources`; `beforeBuildCommand` ist
`npm run build` im Frontend.

**CSP** (`tauri.conf.json`): `default-src 'self'`, `connect-src 'self' ipc:`.

**Gemessen** (Protokoll in der Beilage):
- `cargo-about` 0.9.2, `cargo about generate --format json --all-features`
  mit der `allow`-Liste aus `deny.toml` als `accepted`: Rückgabewert 0,
  33 s, keine Fehler. 705 Pakete, 296 Lizenztexte, Lizenzarten MIT (657),
  Apache-2.0 (26), ISC (21), Unicode-3.0 (19), BSD-3-Clause (8), MPL-2.0 (5),
  CDLA-Permissive-2.0 (2), Zlib (1).
- `cargo metadata`: 739 fremde Pakete, davon eines mit eigener
  Hinweisdatei (`cfg_aliases`, `NOTICES.md`, zwei Versionen).
- npm, Produktionsabhängigkeiten (`npm ls --omit=dev --all`): 107 Pakete,
  alle MIT, ISC, Apache-2.0 oder deren `OR`-Kombination, jedes mit eigener
  Lizenzdatei, keines mit `NOTICE`.

## 2. Teil 0

Teil 0: entfällt — das Werkzeug ist am heutigen Workspace gemessen, und
die npm-Seite braucht kein neues Werkzeug (Lizenzdateien liegen in
`node_modules`).

## 3. Ziel und Nicht-Ziele

Ziel: Lizenztexte entstehen beim Bau aus dem tatsächlichen
Abhängigkeitsbaum, landen im Release-Artefakt und sind in der App lesbar.

Nicht-Ziele:
- Die erzeugte Datei wird **nicht** eingecheckt.
- `NOTICE` und `LICENSE` des Projekts bleiben unverändert.
- Keine Änderung an `deny.toml` und an der Lizenzprüfung selbst.
- Andere Build-Wege, die diese App bauen, sind nicht Teil dieser Spec; sie
  müssen das Skript aus A1 aufrufen können (A1.4).
- Keine Lizenzanzeige für Schriften, Icons oder sonstige Assets außerhalb
  von Cargo und npm.

## 4. Anforderungen

**A1 Erzeugung**
- A1.1 MUSS: Ein Skript im Repo erzeugt **eine** Textdatei mit den
  Lizenzen aller Rust-Abhängigkeiten des Workspace und aller
  Produktionsabhängigkeiten des Frontends. Je Lizenztext stehen die Pakete
  (Name, Version), die ihn verwenden; Pakete des eigenen Workspace dürfen
  fehlen.
- A1.2 MUSS: Hat ein Paket eine Hinweisdatei (`NOTICE*`, Groß/Klein egal),
  steht ihr Inhalt mit Paketname in der Ausgabe.
- A1.3 MUSS: Das Skript bricht mit Rückgabewert ≠ 0 ab, wenn ein Paket
  eine Lizenz hat, die nicht erlaubt ist, wenn für ein Paket kein
  Lizenztext gefunden wird, oder wenn ein Werkzeug fehlt. Es schreibt dann
  keine Ausgabedatei oder entfernt eine halb geschriebene.
- A1.4 MUSS: Ausgabepfad und die Pfade von Workspace und Frontend sind
  Parameter mit den heutigen Pfaden als Vorgabe.
- A1.5 MUSS: Die erlaubten Lizenzen haben **eine** Quelle. Entweder liest
  die Konfiguration sie aus `deny.toml`, oder ein Test in der CI scheitert,
  sobald beide Listen voneinander abweichen.
- A1.6 MUSS: Die Werkzeugversion von `cargo-about` ist fest (Version
  angeben, nicht „latest“), lokal und in der CI dieselbe.
- A1.7 MUSS: Die Ausgabe ist gitignored.

**A2 Release**
- A2.1 MUSS: Jeder Release-Build (alle drei Plattformen der Matrix) erzeugt
  die Datei vor dem App-Bau oder erhält sie aus einem vorgelagerten Job.
- A2.2 MUSS: Schlägt die Erzeugung fehl oder ist die Datei danach leer,
  scheitert der Release-Job; es entsteht kein Artefakt ohne Lizenzen.
- A2.3 MUSS: Die Datei ist Teil des ausgelieferten App-Pakets und ohne
  Netzwerk lesbar.

**A3 CI**
- A3.1 MUSS: Die Community-CI fährt das Skript bei jedem Lauf einmal
  (eine Plattform genügt) und wird rot, wenn es scheitert.

**A4 Anzeige**
- A4.1 MUSS: Der Über-Bereich hat einen Eintrag „Drittanbieter-Lizenzen“
  (EN „Third-party licenses“), der den Text in einer scrollbaren Ansicht
  innerhalb der App öffnet.
- A4.2 MUSS: Der Text wird als reiner Text dargestellt, nie als HTML oder
  Markdown mit eingebettetem HTML.
- A4.3 MUSS: Fehlt die Datei (Entwicklungs-Build), zeigt die Ansicht einen
  Hinweis, dass die Liste nur in Release-Builds enthalten ist, und kein
  leeres Feld und keinen Absturz.
- A4.4 MUSS: Das Laden der Liste löst keine Anfrage an einen fremden Host
  aus.
- A4.5 MUSS: Neue Texte in DE und EN.

**A5 Changelog** — MUSS: Fragment unter `changelog.d/`.

## 5. Design

- Wie die Datei ins Paket kommt (statisches Frontend-Asset, Tauri-Ressource
  oder eingebettet in die Binärdatei) entscheidet der Coder. A2.3, A4.3
  und A4.4 müssen gelten.
- Ob die Release-Matrix selbst erzeugt oder einen vorgelagerten Job nutzt,
  entscheidet der Coder. `cargo-about` sammelt standardmäßig über alle
  Zielplattformen (gemessen: der Lauf auf macOS enthält `windows-sys`,
  `webview2-com` und `gtk`); die Ausgabe hängt also nicht davon ab, auf
  welchem System sie entsteht.
- Die npm-Seite braucht kein neues Paket; Name, Version, Lizenzfeld und
  Lizenzdatei liegen in `node_modules/<paket>/`.

## 6. Sicherheits-Invarianten

- **Fremder Inhalt bleibt Text.** Lizenz- und Hinweisdateien stammen aus
  fremden Paketen. Sie erreichen die Webview nur als Text (A4.2), damit
  kein Paket über seine Lizenzdatei Markup oder Skript einschleust.
- **Kein Netzwerk ohne Nutzeraktion** (BL-0042): A4.4.
- **Lieferkette des Release-Laufs.** Neue Werkzeuge im Release-Workflow
  kommen mit fester Version (A1.6); keine neue Action ohne festen Stand.
- Filter, Redactor, Credentials und Ausführungspfad sind nicht berührt.

## 7. Tests

- **T1 Erzeugung auf dem heutigen Stand** (A1.1): Skript läuft mit
  Rückgabewert 0; die Ausgabe enthält mindestens die Texte zu MIT,
  Apache-2.0 und MPL-2.0 und die Pakete `tauri` und `react`. Scheitert,
  wenn eine der beiden Quellen fehlt.
- **T2 Hinweisdatei** (A1.2): Der Inhalt von `NOTICES.md` aus `cfg_aliases`
  steht in der Ausgabe. Scheitert, wenn Hinweisdateien übergangen werden.
- **T3 Unerlaubte Lizenz** (A1.3, Negativfall): Lauf mit einer erlaubten
  Liste ohne `MIT` endet mit Rückgabewert ≠ 0 und ohne Ausgabedatei.
  Scheitert, wenn das Skript Fehler von `cargo-about` verschluckt oder die
  Datei trotzdem schreibt. Darf von Hand gefahren werden; Befehl und
  Ausgabe in den Bericht.
- **T4 Eine Quelle** (A1.5): Ein zusätzlicher Eintrag nur in `deny.toml`
  (oder nur in der zweiten Liste, falls es eine gibt) macht die CI rot.
  Den Gegenbeweis mit einer verfälschten Kopie in den Bericht.
- **T5 npm vollständig** (A1.1): Die Anzahl der npm-Pakete in der Ausgabe
  entspricht der Anzahl aus `npm ls --omit=dev --all` (heute 107).
  Scheitert, wenn transitive Pakete fehlen.
- **T6 Anzeige mit Datei** (A4.1): Oberflächentest — Klick auf den Eintrag
  öffnet die Ansicht mit dem Text.
- **T7 Anzeige ohne Datei** (A4.3): Laden scheitert → Hinweis „nur in
  Release-Builds“ sichtbar, keine Ausnahme.
- **T8 Markup bleibt Text** (A4.2, adversarial): Ein Lizenztext mit
  `<img src=x onerror=alert(1)>` und `<script>` erscheint wörtlich als
  Text, im DOM entsteht kein `img`- oder `script`-Element. Scheitert bei
  `dangerouslySetInnerHTML` oder einem Markdown-Renderer mit HTML.
- **T9 Kein fremder Host** (A4.4): Der Ladeweg verwendet keine absolute
  `http(s)`-Adresse. Scheitert, wenn die Liste von außen geholt wird.
- **T10 Release-Abbruch** (A2.2): Nicht in der CI ausführbar. Der Coder
  zeigt im Bericht den Workflow-Schritt, der bei Fehlschlag oder leerer
  Datei abbricht, und fährt seine Prüfbedingung lokal einmal mit leerer
  Datei (muss scheitern) und einmal mit der echten (muss durchgehen).
- **T11 Locale-Gleichstand**: bestehender `localeKeyParity.test.ts` grün.

## 8. Offene Punkte

Keine. Entschieden: Erzeugung beim Release-Bau statt eingecheckter Datei.

## 9. Klarstellungen

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `build: generate third-party license notices from the dependency tree [BL-0054]` — A1, T1–T5.
2. `ci: generate the license notices in every release build and in CI [BL-0054]` — A2, A3, T10.
3. `feat(frontend): show third-party licenses in the about settings [BL-0054]` — A4, T6–T9, T11.
4. `docs(changelog): note the third-party licenses in the about settings [BL-0054]` — A5.

**Priorität:** NORMAL. Prüfpunkte für den Review: Fehler von `cargo-about`
werden durchgereicht (T3); die Anzeige rendert nichts als HTML (T8); der
Release-Job kann nicht ohne Datei grün werden (T10).

**Aufteilung:** ein Lauf auf Sonnet. Skript, Workflows und Oberfläche
hängen über den Dateipfad zusammen; kein Sicherheitskern.

**Berührte Module:** neues Skript und Konfiguration im Repo,
`.gitignore`, `.github/workflows/release.yml`,
`.github/workflows/community.yml`, ggf. `tauri.conf.json` und
`apps/smart-ssh-community/build.rs`, Frontend `AboutSettings` und Locales,
`changelog.d/`.

**Melde zurück:** gewählter Weg ins Paket (A2.3) und warum; Größe der
Ausgabedatei; Belege zu T3, T4 und T10; manueller Testablauf (Release-Build
lokal, Über-Bereich öffnen, Dev-Build zeigt den Hinweis).
