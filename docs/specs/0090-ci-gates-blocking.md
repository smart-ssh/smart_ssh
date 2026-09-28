# Spec 0090 — CI gates that actually block

Status: Vorschlag (Architekt) · Backlog: BL-0229, BL-0228, BL-0056, BL-0094, BL-0284, BL-0047 · Gate: release-1.0/E, G, K
Zweck: Was die CI prüft, färbt sie auch rot — Dependency-Audit,
Lizenzprüfung, Frontend-Lint und -Tests. Dazu der offene RustSec-Fund, den
der Berichtsmodus bisher verdeckt.
Review-Priorität: NORMAL

## 1. Ist-Stand (Stand `21e2f0e`, gemessen 2026-09-28 auf macOS)

**Workflow `Community`.** Job `dependency-audit` (nur `ubuntu-latest`):
`cargo deny check licenses sources bans` und `cargo audit`, beide mit
`continue-on-error: true`; die Kommentare darüber nennen den Umstieg auf
blockierend als offenen Folgeschritt. Der Test-Job (Matrix Ubuntu,
Windows, macOS) führt im Frontend nur `npm ci` und `npm run build` aus.
Die Repo-`CLAUDE.md` sagt im Abschnitt zum Gate ausdrücklich, dass die CI
Frontend-Lint und -Tests nicht fährt.

**Gemessen — scharf ausgeführt, ohne `continue-on-error`:**

| Prüfung | Werkzeug | Ergebnis |
|---|---|---|
| `cargo deny check licenses sources bans` | cargo-deny 0.20.2 | Exit 0 („bans ok, licenses ok, sources ok") |
| `cargo audit` | cargo-audit 0.22.2 | **Exit 1**: `RUSTSEC-2026-0285`, `rustls 0.23.43`, Lösung `>=0.23.45` |
| `npm run lint` (oxlint) | Frontend | Exit 0, 4 Warnungen, 0 Fehler |
| `npm test` (vitest) | Frontend | Exit 0, 39 Dateien, 367 Tests |
| `tsc --noEmit --strict` für `tsconfig.app.json` und `tsconfig.node.json` | Frontend | je 0 Fehler (Gegenprobe mit zusätzlich `--exactOptionalPropertyTypes`: 58 Fehler — der Compiler prüft also) |

**TypeScript ist schon heute strikt.** Installiert ist TypeScript 6.0.3;
dort gilt `strict` als Voreinstellung, wenn der Eintrag fehlt. Probe mit
dem Compiler des Repos auf einer Datei mit implizitem `any`: ohne Option
Exit 2, mit `--strict false` Exit 0. Die Aussage in BL-0228 („nicht im
strict-Modus") stammt aus der Zeit vor TypeScript 6.

`rustls` hängt über `hyper-rustls`/`reqwest` an `ai-providers`.
`cargo update -p rustls --dry-run` hebt genau dieses eine Paket auf
0.23.45, semver-kompatibel.

**Ausnahmen in `.cargo/audit.toml`: 18.** Abgleich mit einem `cargo audit`
ohne Ausnahmen auf demselben Lockfile:
- **10 greifen ins Leere:** `RUSTSEC-2024-0411` bis `-0420` (GTK3-Kette).
  Die Crates stehen weiter im Lockfile (`gtk 0.18.2` u. a.), die
  Advisories sind in der RustSec-Datenbank aber zurückgezogen
  (`withdrawn = "2026-08-14"`).
- **8 treffen weiterhin**, darunter die beiden mit Nachprüf-Vermerk
  (`RUSTSEC-2024-0429` glib 0.18.5, `RUSTSEC-2023-0071` rsa).
- Keine der Ausnahmen nennt die Version, gegen die sie entschieden wurde.
- Nur `RUSTSEC-2023-0071` ist eine Sicherheitslücke; die übrigen 7 sind
  Hinweise (`unmaintained`/`unsound`). `cargo audit` endet bei Hinweisen
  und yanked-Crates mit Exit 0 (Messung oben: Exit 1 allein durch 0285,
  wnaf als „allowed warning"). Mit `--deny warnings` bliebe der Lauf
  dauerhaft rot, weil der yanked-Fund `wnaf` keine RUSTSEC-ID hat und sich
  nicht als Ausnahme eintragen lässt (Kommentar am Kopf von `audit.toml`).

Der Abgleich oben lief so: Lockfile in ein Verzeichnis ohne
`.cargo/audit.toml` kopiert, dort `cargo audit --json -f Cargo.lock`,
die gefundenen IDs (Lücken und Hinweise) gegen die Ausnahmeliste
verglichen.

Nebenbefund: `wnaf 0.14.0` (über `p256` ← `russh`) ist yanked; `cargo
audit` meldet das als erlaubte Warnung.

**cargo-deny kann leere Ausnahmen erkennen** (gemessen mit einer
Probe-Konfiguration, die dieselben Ausnahmen unter `[advisories] ignore`
führt): `cargo deny check advisories` meldet
`warning[advisory-not-detected]` je leerer Ausnahme, mit
`-D advisory-not-detected` wird daraus Exit 1. **Abweichung:** cargo-deny
meldete dabei auch `RUSTSEC-2024-0429` als „nicht erkannt", obwohl
`cargo audit` sie auf demselben Lockfile findet (Advisory `informational
= "unsound"`). Die Ursache ist nicht gemessen. Dazu hält `deny.toml`
fest, dass Advisories bewusst nicht dort, sondern nur über `cargo audit`
geprüft werden (Spec 0035). Der cargo-deny-Weg hieße also eine zweite
Ausnahmeliste oder das Ersetzen von `cargo audit` — beides ist hier nicht
gewollt (A4).

## 2. Teil 0

Teil 0: entfällt — alle Prüfungen sind oben scharf gemessen. Nicht
gemessen ist das Verhalten von oxlint und vitest auf den Windows- und
macOS-Runnern der CI (R1).

## 3. Ziel und Nicht-Ziele

Ziel: A1–A7.

Nicht-Ziele:
- Kein Upgrade von GTK3, `russh` oder `rsa`; keine neue Bewertung der
  8 gültigen Ausnahmen (nur ihre Kennzeichnung, A3).
- Keine Behebung der 4 oxlint-Warnungen; keine strengeren Lint-Regeln.
- Hinweise (`unmaintained`/`unsound`) und yanked-Crates blockieren nicht
  (A2); neue Hinweise bleiben sichtbare Warnungen im Protokoll.
- Aus BL-0229 Punkt 3 deckt diese Spec nur ab, dass eine überholte
  Ausnahme auffällt (A3, A4). Die wiederkehrende Nachprüfung von
  `RUSTSEC-2023-0071` bei einem Update von `russh`/`rsa` und die Prüfung
  vor jedem Release liegen außerhalb (BL-0126).
- Keine Änderung an der Test-Matrix oder den Rust-Schritten des
  Test-Jobs. Spec 0089 fasst denselben Workflow an; diese Spec setzt
  auf deren Stand auf.

## 4. Anforderungen

- **A1 MUSS (BL-0284):** Das Lockfile enthält `rustls >= 0.23.45`;
  `cargo audit` endet auf dem Endstand mit Exit 0. Kein anderes Paket
  ändert dabei seine Version.
- **A2 MUSS:** Die Schritte `cargo deny check licenses sources bans` und
  `cargo audit` blockieren: Ein Lizenz-, Herkunfts- oder Bann-Verstoß und
  jede nicht ausgenommene Sicherheitslücke färben den Job rot. Hinweise
  und yanked-Crates bleiben Warnungen (Nicht-Ziele). Kein
  `continue-on-error`, kein `|| true` oder Gleichwertiges.
- **A3 MUSS:** Jede verbleibende Audit-Ausnahme nennt: betroffenes Crate,
  die Version im Lockfile, gegen die entschieden wurde, und woran man
  erkennt, dass sie überholt ist. Die 10 zurückgezogenen Ausnahmen sind
  entfernt.
- **A4 MUSS:** Eine Ausnahme, die auf keinen Befund im Lockfile mehr
  trifft, färbt die CI rot. „Befund" heißt: was `cargo audit` ohne
  Ausnahmen meldet, Lücken **und** Hinweise (so wie der Abgleich in §1).
  Die Ausnahmeliste bleibt in `.cargo/audit.toml`, an einer Stelle.
  Der Aufruf ohne Ausnahmen endet erwartungsgemäß mit Exit 1 (0071);
  ausgewertet wird seine JSON-Ausgabe. Fehlt sie oder lässt sie sich nicht
  lesen, ist die Prüfung rot. Das Verbot aus A2 gilt für die beiden
  bestehenden Schritte, nicht für diesen Zwischenaufruf.
- **A5 MUSS:** Der Test-Job führt im Frontend nach `npm ci` auch
  `npm run lint` und `npm test` aus; ein Fehler in einem der beiden färbt
  den Job rot. Warnungen von oxlint bleiben Warnungen. Ein fehlendes
  `node_modules` wird dabei von selbst rot, weil `npm ci` vorausgeht und
  `npm run` ohne das Werkzeug mit Exit ≠ 0 endet; das lokale Gate ist
  nicht Teil dieser Spec.
- **A6 MUSS:** `"strict": true` steht ausdrücklich in `tsconfig.app.json`
  und `tsconfig.node.json`. Das ändert heute nichts (§1), schreibt den
  Zustand aber fest, falls sich die Voreinstellung des Compilers ändert.
  `npm run build` bleibt grün.
- **A7 MUSS:** Die Repo-`CLAUDE.md` beschreibt, was die CI nach dieser
  Spec prüft (Frontend-Lint und -Tests laufen dort; Audit und Lizenzen
  blockieren). Kommentare in Workflow, `.cargo/audit.toml` und `deny.toml`,
  die den Berichtsmodus als Zustand oder offenen Folgeschritt beschreiben
  oder nach A5 nicht mehr stimmen (etwa der Kopfkommentar des Workflows
  zum Frontend-Build), sind entfernt oder berichtigt.

## 5. Design

Nichts vorzugeben.

## 6. Sicherheits-Invarianten

Keine Sicherheitslogik wird geändert. A1 hebt eine TLS-Bibliothek im Pfad
zu den KI-Anbietern um zwei Patch-Versionen; Verhalten und Konfiguration
der Verbindungen bleiben unverändert.

## 7. Tests

Kein neuer Produkttest. Nachweise, jeweils mit Befehl und Rückgabewert im
Bericht; die Negativproben laufen auf einer Wegwerf-Änderung, die **nicht**
committet wird. Sie fahren lokal genau den Befehl aus dem Workflow; einen
roten CI-Lauf als Gegenprobe gibt es nicht, weil jeder CI-Lauf einen Push
voraussetzt. Die CI-Seite belegt der Workflow-Diff.

- **N1 (A1):** `cargo audit` → Exit 0; `cargo tree -i rustls` zeigt
  ≥ 0.23.45; `git diff` am Lockfile ändert nur `rustls` (Version und
  Prüfsumme).
- **N2 (A2, Audit):** Workflow-Diff zeigt den Audit-Schritt ohne
  `continue-on-error`/`|| true`. Negativ: die Ausnahme `RUSTSEC-2023-0071`
  (die einzige Lücke unter den Ausnahmen) vorübergehend entfernen → der
  Audit-Befehl aus dem Workflow endet mit Exit ≠ 0.
- **N3 (A2, Lizenzen):** Workflow-Diff zeigt den cargo-deny-Schritt ohne
  `continue-on-error`/`|| true`. Negativ: eine Crate mit GPL-Lizenz
  vorübergehend als Abhängigkeit eines Workspace-Crates eintragen →
  `cargo deny check licenses` endet mit Exit ≠ 0. (Deckt BL-0056.)
- **N4 (A4, negativ):** Eine der 10 zurückgezogenen Ausnahmen
  vorübergehend wieder eintragen → die Prüfung aus A4 endet mit Exit ≠ 0;
  ohne sie Exit 0.
- **N5 (A4, Gegenprobe):** Die Prüfung aus A4 ist auf dem Endstand mit
  allen 8 gültigen Ausnahmen grün — insbesondere wird `RUSTSEC-2024-0429`
  nicht als leer gemeldet.
- **N6 (A5):** Workflow-Diff zeigt `npm run lint` und `npm test` im
  Test-Job, ohne `continue-on-error`. Negativ: ein absichtlich
  fehlschlagender vitest-Fall → `npm test` Exit ≠ 0; ein Verstoß gegen
  eine oxlint-Regel der Stufe „error" (etwa ein Hook-Aufruf in einer
  Bedingung, `react/rules-of-hooks`) → `npm run lint` Exit ≠ 0.
- **N7 (A6):** Diff zeigt `"strict": true` in beiden Dateien; `npm run
  build` Exit 0. Gegenprobe: eine exportierte Funktion, deren Parameter
  implizit `any` ist und benutzt wird → mit vorübergehend `"strict": false`
  Build grün, mit dem Eintrag Build rot.
- **N8:** Gate der Repo-`CLAUDE.md` vollständig grün.
- **N9 (A3, A7):** Diff von `.cargo/audit.toml` zeigt je verbleibender
  Ausnahme Crate, Version und Ablaufbedingung; Diff von `CLAUDE.md`,
  Workflow und Konfigurationsdateien zeigt die berichtigten Stellen.

## 8. Offene Punkte

Keine.

**R1 (bewusst offen):** oxlint und vitest liefen nie auf Windows- oder
macOS-Runnern der CI. Scheitern sie dort, zeigt das erst der CI-Lauf nach
dem Push; die Behebung folgt dann mit eigenem Lauf. BL-0047 gilt erst als
erledigt, wenn der Workflow `Community` auf dem Endstand in allen Jobs
`success` meldet; das setzt voraus, dass BL-0281 (Spec 0089) erledigt
ist.

**R2 (bewusst offen):** Ab A2 kann ein neu veröffentlichtes Advisory einen
Lauf rot färben, ohne dass sich am Code etwas geändert hat — ebenso ein
zurückgezogenes Advisory, dessen Ausnahme dann ins Leere zeigt (A4;
Abhilfe: Ausnahme entfernen). Das ist der Zweck der Änderung.

## 9. Klarstellungen

(leer)

## Umsetzung

**Teil 0:** entfällt.

**Voraussetzung:** Spec 0089 ist auf `main` umgesetzt (gleicher Workflow).

**Reihenfolge:**
1. `fix(deps): update rustls to 0.23.45 for RUSTSEC-2026-0285 [BL-0284]` — A1.
2. `ci: make the dependency audit and license check blocking [BL-0229, BL-0056, BL-0094]` — A2–A4.
3. `ci(frontend): run lint and tests in CI, pin strict TypeScript [BL-0228]` — A5, A6.
4. `docs: describe what CI checks [BL-0229, BL-0228]` — A7.

**Priorität:** NORMAL.

**Aufteilung:** ein Lauf, Sonnet. Kleine Änderungen an Konfiguration und
Workflow, kein Produktcode außer dem Lockfile.

**Berührte Dateien:** `Cargo.lock`, `.cargo/audit.toml`, `deny.toml` (nur
Kommentare, falls betroffen),
`.github/workflows/community.yml`,
`apps/smart-ssh-community/frontend/tsconfig.app.json`,
`apps/smart-ssh-community/frontend/tsconfig.node.json`, `CLAUDE.md`.

**Melde zurück:** N1–N9 je mit Befehl und Exit-Code; den Befehl
der Prüfung aus A4.
