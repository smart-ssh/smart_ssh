# Spec 0070 — Lizenzwechsel FSL-1.1-MIT → Apache License 2.0

Status: Vorschlag (Architekt) · Backlog: BL-0055 · Gate: Release-Gate 1.0, G
Repo: **öffentlich** `smart_ssh` — Repo-Wurzel (`LICENSE*`, `NOTICE`,
`README.md`, `CHANGELOG.md`, `Cargo.toml`, `deny.toml`), Frontend-Manifest
(`apps/smart-ssh-community/frontend/package.json`, `package-lock.json`),
neuer ADR in `docs/adr/`
Review-Priorität: normal (kein Code-Pfad, keine Sicherheits-Invariante berührt)

> Das Repo wechselt von der Functional Source License 1.1 (MIT Future
> License) auf die **Apache License 2.0**. Reine Deklarations- und
> Dokumentationsänderung: kein Rust- oder TypeScript-Code ändert sich.
> Der Wechsel ist **nach der Veröffentlichung nicht umkehrbar** (einmal
> unter Apache 2.0 veröffentlichter Code bleibt unter Apache 2.0 nutzbar) —
> deshalb genau und vollständig, aber ohne Nebenbaustellen.

## Getroffene Entscheidungen (Stefan, 2026-09-22)

- Ziel-Lizenz des gesamten öffentlichen Repos: **Apache License 2.0**,
  SPDX-Kennung `Apache-2.0`.
- Externe Beitragende: keine (alle Commits vom Rechteinhaber, geprüft).
  Eine Zustimmung Dritter ist nicht nötig.
- Umsetzung jetzt, vor dem Launch.

Alles Weitere in dieser Spec ist Vorschlag des Architekten; die Punkte, die
Stefan entscheiden muss, stehen in §8.

---

## 1. Ausgangslage (belegt, Stand dieses Worktrees)

Vollständige Suche nach `FSL`, `Functional Source`, `MIT Future`,
`source-available`, `license`/`Lizenz`, `Copyright`, `SPDX` im ganzen Repo.
Echte Fundstellen:

| Datei | Stelle | Inhalt heute |
|---|---|---|
| `LICENSE.md` | ganze Datei | FSL-1.1-MIT-Text mit `Copyright 2026 Stefan Richter` |
| `Cargo.toml` | Z. 8 | `[workspace.package] license = "FSL-1.1-MIT"` |
| `Cargo.toml` | Z. 9–12 | Kommentar zu `publish = false`: „(proprietäre App)" |
| `deny.toml` | Z. 72–79 | Kommentar zu `[licenses.private] ignore = true`, nennt `FSL-1.1-MIT` |
| `README.md` | Z. 5 | Badge `License: MIT`, verlinkt auf `LICENSE` — **Datei existiert nicht** (toter Link) |
| `README.md` | Z. 134–136 | Abschnitt „Lizenz": FSL-1.1-MIT, Link auf `LICENSE.md` |

Alle acht Workspace-Crates (`crates/*`, `apps/smart-ssh-community`) erben
die Lizenz per `license.workspace = true` — eine Änderung in `Cargo.toml`
genügt.

**Keine Fundstelle** (geprüft, vom Backlog-Hinweis abweichend):

- `crates/core/src/filter/pattern.rs` (Z. 111/113), `docs/adr/0053-…`
  (Z. 208–213) und `docs/adr/0054-…` (Z. 24) sind **Fehlalarme**: eine
  Suche ohne Groß-/Kleinschreibung nach `fsl` trifft das Wort
  „Nachlau**fsl**ash". Dort steht nichts zur Lizenz. **Nicht anfassen.**
- `package-lock.json` enthält weder `FSL` noch ein `license`-Feld für das
  eigene Paket; `package.json` hat kein `license`-Feld (nur `"private": true`).
- Dateikopfzeilen: **Keine einzige** Quelldatei hat einen Lizenz-, SPDX-
  oder Copyright-Kopf. Es gibt also nichts auszutauschen (siehe §8, OP-1).
- `tauri.conf.json`, `.github/workflows/*`, `README_DEV.md`, Locale-Dateien,
  Über-Dialog: keine Lizenzangabe.
- Historische Specs (`docs/specs/0001` Z. 120 u. a.) erwähnen die Lizenz nur
  als damalige Frage; sie bleiben unverändert.

## 2. Ziel und Nicht-Ziele

**Ziel:** Nach dem Merge deklariert jede Stelle im Repo, die eine Lizenz
für dieses Projekt nennt, übereinstimmend `Apache-2.0`, und der
maßgebliche Lizenztext ist der unveränderte offizielle Apache-2.0-Text.

**Nicht-Ziele** (nicht „mitreparieren"):

- Drittlizenzen/Attributions-Sammlung der Abhängigkeiten (`cargo-about`,
  Über-Dialog) — das ist BL-0054. Diese Spec legt nur fest, dass `NOTICE`
  dafür **nicht** der Sammelort ist (§4.3).
- CONTRIBUTING.md, CLA/DCO — eigener Gate-Punkt.
- Website-Lizenzhinweis — eigener Punkt (BL-0059), außerhalb dieses Repos.
- Installer-Metadaten/Lizenzseiten in Tauri-Bundles (`tauri.conf.json`
  `bundle.license`/`licenseFile`) — heute ohne Angabe, also nicht falsch;
  eine Lizenzseite im Installer wäre eine nutzersichtbare Änderung.
- `publish = false`, `[licenses.private] ignore = true`, die Allow-Liste in
  `deny.toml` — bleiben inhaltlich unverändert (§4.5).
- Aktualisierung nachgelagerter Konsumenten dieses Repos (Submodule).
- Sonstige veraltete README-Stellen (z. B. Spec-/ADR-Nummernbereiche in
  Z. 64/65/129/130) — nicht Teil dieser Spec.
- Historische Specs und ADRs — werden nicht umgeschrieben.

## 3. Anforderungen

- **3.1 (MUSS)** `LICENSE` (ohne Endung) im Repo-Wurzelverzeichnis enthält
  den offiziellen Text der Apache License 2.0 **Byte für Byte unverändert**,
  bezogen von `https://www.apache.org/licenses/LICENSE-2.0.txt`, inklusive
  des Abschnitts „APPENDIX: How to apply the Apache License to your work"
  mit seinen Platzhaltern (`[yyyy] [name of copyright owner]`) —
  **Platzhalter nicht ausfüllen** (sie sind eine Vorlage für Dateiköpfe,
  kein Teil der Lizenzerteilung).
- **3.2 (MUSS)** `LICENSE.md` ist gelöscht. Kein Verweis auf `LICENSE.md`
  bleibt außerhalb von `docs/specs/`, `docs/adr/` und `CHANGELOG.md`.
- **3.3 (MUSS)** `Cargo.toml`: `license = "Apache-2.0"`. Alle acht
  Workspace-Pakete melden über `cargo metadata` `Apache-2.0`.
- **3.4 (MUSS)** Kommentar in `Cargo.toml` Z. 9–12: „(proprietäre App)"
  ersetzen durch eine sachliche Begründung ohne Lizenzaussage, z. B.
  „keine dieser Crates wird auf crates.io veröffentlicht". Der Verweis auf
  `[licenses.private] ignore = true` bleibt.
- **3.5 (MUSS)** Kommentar in `deny.toml` Z. 72–79: `FSL-1.1-MIT` durch
  `Apache-2.0` ersetzen und die Begründung anpassen (§4.5). Keine
  Änderung außerhalb von Kommentarzeilen in `deny.toml`.
- **3.6 (MUSS)** `package.json` erhält `"license": "Apache-2.0"` (direkt
  nach `"version"`). Der Lockfile-Wurzeleintrag (`packages[""]`) enthält
  danach ebenfalls `"license": "Apache-2.0"`; sonst ändert sich im
  Lockfile **nichts** (§4.4).
- **3.7 (MUSS)** `README.md`: Badge Z. 5 zeigt „Apache 2.0" und verlinkt auf
  `LICENSE`; Abschnitt „Lizenz" (Z. 134–136) nennt Apache License 2.0 mit
  Link auf `LICENSE` (und, falls OP-2 so entschieden, auf `NOTICE`).
  Wortlaut §4.6.
- **3.8 (MUSS)** `CHANGELOG.md`, `[Unreleased]` → `### Changed`: ein
  Eintrag zum Lizenzwechsel, Wortlaut §4.7 (abhängig von OP-3).
- **3.9 (MUSS, abhängig von OP-2)** `NOTICE` im Repo-Wurzelverzeichnis mit
  exakt dem Inhalt aus §4.3.
- **3.10 (MUSS)** ADR `docs/adr/0060-license-apache-2-0.md` (nächste freie
  Nummer zum Commit-Zeitpunkt prüfen) mit dem Inhalt aus §4.8.
- **3.11 (MUSS)** Nach der Änderung liefert die case-sensitive Suche aus
  §6 T1 keinen Treffer außerhalb der erlaubten Stellen.
- **3.12 (MUSS)** Kein Rust-/TypeScript-Quelltext ändert sich
  (`git diff --stat` enthält keine `.rs`, `.ts`, `.tsx`, `.css`).

## 4. Design

### 4.1 Dateiname `LICENSE` statt `LICENSE.md`

Der Apache-Text ist Klartext mit fester Einrückung; als Markdown gerendert
würden Nummerierung und Einzüge verfälscht. `LICENSE` ohne Endung ist die
Konvention, wird von GitHubs Lizenzerkennung sicher erkannt, und der
README-Badge verlinkt bereits heute auf `LICENSE`. Verworfen: `LICENSE.md`
mit Apache-Text (Rendering), `LICENSE.txt` (kein Vorteil, Badge-Link
müsste sich ändern).

### 4.2 Lizenztext unverändert übernehmen

Keine eigenen Ergänzungen im Lizenztext (kein Copyright-Kopf oben, keine
ausgefüllten Appendix-Platzhalter). Der Copyright-Vermerk, der heute im
FSL-Text steht (`Copyright 2026 Stefan Richter`), wandert in `NOTICE`
(OP-2). Nicht aus dem Gedächtnis oder aus einer Crate-Kopie im
Cargo-Registry-Cache übernehmen — dort kursieren Varianten mit
ausgefüllten Platzhaltern oder anderen Zeilenumbrüchen.

### 4.3 `NOTICE` und Verhältnis zu BL-0054 (Vorschlag, OP-2)

Apache 2.0 §4(d) verlangt von Weiterverteilern, den Inhalt einer
vorhandenen `NOTICE`-Datei mitzuliefern. Vorschlag: eine minimale `NOTICE`,
die nur den eigenen Vermerk trägt:

```
Smart SSH
Copyright 2026 Stefan Richter
```

(genau zwei Zeilen, abschließender Zeilenumbruch, keine weiteren Inhalte).

**Abgrenzung zu BL-0054:** `NOTICE` ist **kein** Sammelort für
Drittlizenzen. Nach Apache-Konvention gehören dort nur rechtlich
erforderliche Vermerke hinein; jede Zeile darin wird für alle
Weiterverteiler zur Pflicht. Die generierte Liste der Drittlizenzen
(BL-0054) wird ein eigenes Artefakt (Name legt BL-0054 fest). Hinweis für
BL-0054, nicht Teil dieser Spec: Abhängigkeiten unter Apache 2.0, die
selbst eine `NOTICE`-Datei mitbringen, verlangen deren Wiedergabe beim
Weiterverteilen des Binaries — ob `cargo-about` das abdeckt, ist dort zu
verifizieren.

### 4.4 Frontend-Manifest

`"license"` in `package.json` setzen, dann den Lockfile aktualisieren mit
`npm install --package-lock-only --ignore-scripts` im Frontend-Ordner.
Danach `git diff package-lock.json` prüfen: erwartet ist **genau eine
hinzugefügte Zeile** im Eintrag `packages[""]`. Enthält der Diff mehr
(andere npm-Version schreibt Einträge um), die Lockfile-Änderung
verwerfen und die eine Zeile `"license": "Apache-2.0",` im Eintrag
`packages[""]` nach `"version": "0.5.0",` von Hand einfügen. Anschließend
muss `npm ci` fehlerfrei durchlaufen. Begründung für das Feld trotz
`"private": true`: eine einzige, maschinenlesbare Lizenzangabe pro Manifest,
passend zu Cargo; Werkzeuge (SBOM, Lizenzscanner) lesen es.

### 4.5 `deny.toml` / `Cargo.toml`

`[licenses.private] ignore = true` bleibt. Mit `Apache-2.0` würde die
eigene Lizenz zwar auch die Allow-Liste passieren, aber die Trennung „die
Allow-Liste gilt nur für Drittabhängigkeiten" bleibt richtig und ist
bewährt; das Entfernen wäre eine unnötige Konfigurationsänderung an einem
CI-Gate. Nur der Kommentar wird sachlich nachgezogen, sinngemäß:

> Spec 0035 gilt für Abhängigkeiten, nicht für die eigene Lizenz des
> Projekts (`[workspace.package] license = "Apache-2.0"`, Spec 0070) — …
> kein Eintrag der eigenen Lizenz in die Allow-Liste, die weiterhin
> ausschließlich für Drittanbieter-Abhängigkeiten gilt.

Die Allow-Liste selbst ändert sich nicht (Apache-2.0 steht dort bereits,
als Drittlizenz).

### 4.6 README

- Z. 5: `[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)`
- Abschnitt „📄 Lizenz" ersetzen durch (README ist deutschsprachig):

  > Smart SSH steht unter der **Apache License 2.0** – siehe
  > [LICENSE](LICENSE). Urheberrechtsvermerk: [NOTICE](NOTICE).

  Den zweiten Satz nur, wenn `NOTICE` angelegt wird (OP-2). Kein Hinweis
  auf frühere Lizenzen im README (das README beschreibt den Ist-Zustand;
  die Historie steht im CHANGELOG).

### 4.7 CHANGELOG

Unter `[Unreleased]` → `### Changed` als erster Eintrag (Vorschlag gemäß
Empfehlung zu OP-3):

> - Smart SSH ist jetzt Open Source unter der **Apache License 2.0**
>   (bisher Functional Source License 1.1, FSL-1.1-MIT). Der Wechsel gilt
>   ab dieser Version; bereits veröffentlichte Versionen behalten ihre
>   bisherige Lizenz.

### 4.8 ADR 0060 (vom Coder zu schreiben)

Format nach `docs/adr/README.md`, kurz:

- **Status:** Accepted
- **Kontext:** Repo bisher unter FSL-1.1-MIT (Non-Compete, Umwandlung in
  MIT nach zwei Jahren; keine OSI-anerkannte Open-Source-Lizenz). Ziel:
  echte Open-Source-Lizenz vor dem 1.0-Launch.
- **Entscheidung:** Apache License 2.0 (SPDX `Apache-2.0`) für das gesamte
  Repo. Gründe in einem Satz je Punkt: OSI-anerkannt; ausdrückliche
  Patentlizenz; in Unternehmen ohne Rechtsprüfung einsetzbar; kompatibel
  mit der bestehenden `cargo deny`-Allow-Liste. Keine externen
  Beitragenden, daher keine Zustimmung nötig. Dateiname `LICENSE`;
  Umgang mit `NOTICE` und Dateikopfzeilen gemäß Entscheidung zu OP-1/OP-2.
- **Konsequenzen:** unumkehrbar für veröffentlichten Code; frühere
  Versionen behalten ihre Lizenz (gemäß OP-3); ADRs/Specs, die FSL
  erwähnen, bleiben als historische Dokumente unverändert.

Keine Aussagen zu Editionen oder Geschäftsmodell im ADR.

## 5. Sicherheits-Invarianten

Keine berührt: Es ändert sich weder Code noch Laufzeitverhalten.
Einzige Berührung eines Schutzmechanismus ist `deny.toml` (Lizenz-Gate in
CI). Invariante dort: **Allow-Liste und alle Nicht-Kommentar-Zeilen
bleiben unverändert** — geprüft durch T4. Insbesondere wird keine Lizenz
hinzugefügt und `[licenses.private]` nicht angefasst.

## 6. Tests / Prüfungen

Kein Unit-Test sinnvoll (keine Logik). Stattdessen reproduzierbare
Prüfungen, deren Ausgabe der Coder in seinen Bericht übernimmt:

- **T1 Restfundstellen:** `rg -n 'FSL|Functional Source|MIT Future'`
  (**case-sensitive** — ohne `-i`, sonst Fehlalarme auf „Nachlaufslash")
  liefert Treffer nur in `docs/specs/`, `docs/adr/` und dem neuen
  CHANGELOG-Eintrag. *Scheitert*, wenn eine Deklaration übersehen wurde.
- **T2 Lizenztext:** frisch geladenen offiziellen Text mit `LICENSE`
  vergleichen: `curl -fsSL https://www.apache.org/licenses/LICENSE-2.0.txt | diff - LICENSE`
  → leere Ausgabe. *Scheitert* bei ausgefüllten Platzhaltern, verlorenen
  Leerzeilen oder CRLF-Zeilenenden. Die Datei mit LF einchecken;
  `.gitattributes` **nicht** ändern (bewusst nur `*.rs`, wegen der
  Migrations-Prüfsummen — siehe Kommentar dort).
- **T3 Cargo:** `cargo metadata --format-version 1 --no-deps` → alle acht
  Pakete mit `"license":"Apache-2.0"`. *Scheitert*, wenn eine Crate die
  Lizenz nicht erbt.
- **T4 Lizenz-Gate unverändert:** `git diff deny.toml` zeigt nur Zeilen,
  die mit `#` beginnen; `cargo deny check licenses sources bans` ist grün.
  *Scheitert*, wenn die Allow-Liste oder `[licenses.private]` verändert
  wurde.
- **T5 Frontend:** `git diff package-lock.json` = genau eine hinzugefügte
  Zeile in `packages[""]`; `npm ci` fehlerfrei; `npm pkg get license`
  → `"Apache-2.0"`.
- **T6 Links:** `rg -n 'LICENSE\.md'` ohne Treffer außerhalb von
  `docs/specs/`, `docs/adr/`, `CHANGELOG.md`; `LICENSE` (und ggf. `NOTICE`)
  existieren; der Badge-Link in README Z. 5 zeigt auf eine vorhandene Datei.
- **T7 Kein Code berührt:** `git diff --stat main...HEAD` enthält keine
  `.rs`/`.ts`/`.tsx`/`.css`-Dateien.
- **T8 Vollständiges Gate** aus `CLAUDE.md` (cargo fmt/clippy/test, tsc,
  oxlint, vitest) grün — Absicherung gegen versehentliche
  Manifest-Syntaxfehler.
- **Manuell (Stefan, nach dem Push):** GitHub zeigt in der Repo-Seitenleiste
  „Apache-2.0 license"; README-Badge und Lizenz-Link funktionieren.

## 7. Umsetzungsreihenfolge

Voraussetzung: Entscheidungen zu OP-1 bis OP-3 liegen vor (fallen mit
Tor 1 zusammen).

1. **Ein Commit für den eigentlichen Wechsel**, damit es keinen
   Zwischenstand gibt, in dem Lizenztext und Deklarationen
   widersprechen: `LICENSE` neu, `LICENSE.md` gelöscht, ggf. `NOTICE`,
   `Cargo.toml`, `deny.toml` (Kommentar), `package.json`,
   `package-lock.json`, `README.md`, `CHANGELOG.md`.
   Vorschlag: `chore(license): relicense under Apache-2.0 (spec 0070)`.
2. **ADR separat:** `docs(adr): record license change to Apache-2.0 (spec 0070)`.
3. Diese Spec mit dem ADR committen (Spec-first-Regel in `CLAUDE.md`).

Pfade einzeln stagen, kein `git add -A`. Kein Push — die Veröffentlichung
(und damit das Wirksamwerden) liegt bei Stefan.

## 8. Offene Punkte (Entscheidung Stefan)

**OP-1 — Dateikopfzeilen (K3).** Das Release-Gate nennt „Dateikopfzeilen";
es gibt aber keine (§1). Optionen:
1. *Keine Kopfzeilen* (Ist-Zustand beibehalten). Lizenz ist zentral über
   `LICENSE`, `NOTICE` und die Manifest-Felder deklariert. Apache 2.0
   verlangt keine Dateiköpfe (der Appendix ist eine Empfehlung).
2. *Einzeiliger SPDX-Kopf* `// SPDX-License-Identifier: Apache-2.0` in
   allen eigenen `.rs`/`.ts`/`.tsx`-Dateien, plus CI-Prüfung, dass neue
   Dateien ihn tragen. Folge: Änderung an praktisch jeder Quelldatei —
   Merge-Konflikte mit allen parallel laufenden Worktrees; neue
   CI-Prüfung.
3. *Voller Apache-Boilerplate-Kopf* je Datei. Wie 2, nur deutlich länger.

**Empfehlung: 1.** Kein rechtlicher Mehrwert gegenüber einer eindeutigen
`LICENSE` im Wurzelverzeichnis, erheblicher Konfliktaufwand jetzt. Option 2
bleibt jederzeit später nachrüstbar, ohne etwas zu brechen. Der
Gate-Unterpunkt „Dateikopfzeilen" gilt damit als „geprüft, keine
vorhanden, bewusst nicht eingeführt" — da das den Wortlaut des Gates
nicht wörtlich erfüllt, ist das als mögliche Scope-Reduktion
dokumentiert und braucht Stefans Entscheidung.

**OP-2 — `NOTICE`-Datei (K3, rechtliche Wirkung).** Optionen:
1. *Minimale `NOTICE`* mit Projektname und Copyright-Vermerk (§4.3).
   Folge: Weiterverteiler müssen den Vermerk mitliefern; der bisher im
   FSL-Text stehende Copyright-Vermerk bleibt erhalten.
2. *Keine `NOTICE`*, Copyright-Vermerk nur im README. Folge: keine
   Mitlieferpflicht für den Vermerk über die Lizenz selbst hinaus.
3. *`NOTICE` inklusive Drittlizenzen.* Verworfen: widerspricht der
   Apache-Konvention und macht jede Zeile zur Pflicht aller Weiterverteiler;
   Drittlizenzen gehören in BL-0054.

**Empfehlung: 1** — übliche Apache-Praxis, erhält den bestehenden
Copyright-Vermerk, wirkt ohne Dateiköpfe (OP-1) als einziger
Urheberrechtsvermerk im Repo.

**OP-3 — Aussage zu bereits veröffentlichten Versionen (K3, rechtliche
Wirkung).** Optionen:
1. *Nur ab jetzt:* CHANGELOG sagt, dass der Wechsel ab dieser Version gilt
   und frühere Versionen ihre Lizenz behalten (§4.7).
2. *Rückwirkend:* zusätzlich erklären, dass auch alle früheren Versionen
   unter Apache 2.0 genutzt werden dürfen (als alleiniger Rechteinhaber
   möglich; unwiderruflich).
3. *Keine Aussage:* CHANGELOG nennt nur den Wechsel.

**Empfehlung: 1** — sachlich korrekt, erteilt keine zusätzlichen Rechte,
die nicht verlangt sind; Option 2 lässt sich jederzeit nachholen, aber
nicht zurücknehmen.

Solange OP-1 bis OP-3 offen sind, ist die Spec **nicht umsetzbar** (sie
bestimmen, welche Dateien entstehen und welcher Wortlaut öffentlich wird).

## 9. Klarstellungen

(wird während der Umsetzung nachgetragen: Datum · Frage-ID · Antwort)
