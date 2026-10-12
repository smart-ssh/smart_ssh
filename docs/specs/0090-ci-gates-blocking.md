# Spec 0090 — CI-Prüfungen und Release-Gate

Status: umgesetzt
Zweck: Was die CI prüft, färbt sie auch rot. Jeder Pull Request und jeder
Stand von `main` durchläuft dieselben Prüfungen auf Linux, Windows und
macOS, dazu eine Prüfung der Abhängigkeiten. Ein Release entsteht nur aus
einem Commit, auf dem genau diese Prüfungen grün sind.
Bezüge: Spec 0099 (Drittlizenzen), Spec 0084 (Tauri-freie
Anwendungslogik), ADR 0028 (Ausnahme RUSTSEC-2023-0071), ADR 0083
(Entscheidungen zu blockierenden Prüfungen), ADR 0085 und ADR 0122
(plattform- und zeitunabhängige Tests).

Diese Spec fasst die früheren Specs 0035, 0089, 0091 (CI-Teil), 0093 und
0097 (Gate-Teil) zusammen.

## 1. Wann die CI läuft

- Bei jedem Pull Request und bei jedem Push auf einen der langlebigen
  Branches: `main` (die veröffentlichte Version), `develop` (die nächste
  Version), `release/*` (eine Version im Test) und `hotfix/*` (eine
  dringende Korrektur der veröffentlichten Version). Ein Commit eines
  Pull-Request-Branches läuft einmal, nicht doppelt unter denselben
  Prüfnamen.
- Ein neuer Push auf denselben Pull Request bricht dessen noch laufenden
  Lauf ab. Push-Läufe auf den langlebigen Branches brechen sich gegenseitig
  nicht ab.
- Die CI baut aus einem frischen Klon ohne Secrets. Scheitert ein Schritt,
  weil etwas außerhalb dieses Repositorys fehlt, ist das ein Fehler.

## 2. Test-Job

Läuft auf Linux, Windows und macOS. Ein rotes Betriebssystem bricht die
anderen nicht ab, jedes meldet sein eigenes Ergebnis.

- **A9 — Rust.** In dieser Reihenfolge, jeder Schritt blockierend:
  Formatprüfung (`cargo fmt --all --check`), Clippy über alle Ziele mit
  Warnungen als Fehler, alle Tests des Workspace, Build des Workspace ohne
  Test-Features. Der Testschritt läuft bis zum Ende weiter, auch wenn ein
  Test-Binary scheitert, damit ein Lauf alle scheiternden Ziele zeigt. Der
  Build ohne Test-Features fängt Produktivcode, der einen nur für Tests
  gedachten Zugang aufruft.
- **A10 — Grenzprüfung.** Die Tauri-freie Anwendungslogik (Spec 0084) hat
  in ihrem gesamten Abhängigkeitsbaum — direkt oder transitiv, normale,
  Build- und Dev-Abhängigkeiten, unter allen Features — weder Tauri noch
  ein Tauri-Paket, `wry`, `tao` oder `rfd`. Die Prüfung läuft auf allen
  drei Betriebssystemen. Lässt sich der Baum nicht ermitteln, ist der
  Schritt rot, nicht grün.
- **A11 — Drittlizenzen.** Die Liste der Drittlizenzen (Spec 0099) wird auf
  allen drei Betriebssystemen erzeugt; scheitert die Erzeugung, ist der Job
  rot.
- **A5 — Frontend.** Nach der Installation der Abhängigkeiten laufen Lint,
  Tests und Build. Ein Lint-Verstoß der Stufe „error" und ein
  fehlschlagender Test färben den Job rot; Lint-Warnungen bleiben
  Warnungen. Der Lint-Schritt prüft zuerst, dass jedes Backend-Kommando,
  das das Frontend aufruft, registriert ist.
- **A6 — Strenges TypeScript.** Das Frontend wird im strikten Modus von
  TypeScript übersetzt; das steht ausdrücklich in der Konfiguration und
  hängt nicht von der Voreinstellung des Compilers ab.

## 2a. Start der echten App

Eigener Job, nur auf Linux. Er baut die Community-App und startet sie
tatsächlich — nicht gegen ein nachgebautes Backend (Browser-Tests) und nicht
ohne Tauri (Rust-Tests). Er fängt einen Start, der seinen eigenen Zustand
fallen lässt, bevor ein Release entsteht (Issue #234: im Schlüsselbund-Modus
scheiterte jedes Kommando, das Fenster zeigte keine Server).

- **A14 — Drei Startpfade.** Je Pfad startet die App auf einem eigenen,
  vorbereiteten Datenverzeichnis, in einer Sitzung mit einem entsperrten
  Test-Schlüsselbund und einer virtuellen Anzeige:
  1. frisches Datenverzeichnis im Schlüsselbund-Modus,
  2. Aktualisierung: eine von Version 0.5.2 geschriebene Klartext-Datenbank
     mit den zugehörigen Zugangsdaten im Schlüsselbund,
  3. Passwort-Modus: gesperrter Start, Entsperren im Fenster, Liste.
- **A15 — Was jeder Pfad prüft.** Das Fenster listet die erwarteten
  Server, und kein Kommando wird mit einem Zustands- oder Sperrfehler
  abgelehnt. Im Passwort-Modus wird vor dem Entsperren jedes Kommando
  abgelehnt und danach beantwortet. Scheitert der Start der App, der
  Treiber oder eine Erwartung, ist der Job rot; ein Pfad wird nie
  übersprungen.
- **A16 — Testdaten.** Die Fixtures enthalten ausschließlich erkennbar
  erfundene Werte, keine echten Zugangsdaten. Die Aktualisierung geht von
  einer Datei aus, die das Release selbst geschrieben hat, nicht von einer
  mit dem heutigen Code erzeugten.
- **A17 — Wann.** Auf `main`, `develop`, `release/*` und `hotfix/*` immer;
  bei einem Pull Request, wenn er den Start der App, die Anwendungslogik
  des Starts oder den Einstiegspunkt des Frontends berührt. Der Job bleibt
  kurz: ein Build, drei Starts.

## 3. Prüfung der Abhängigkeiten

Ein eigener Job, nur auf Linux (das Ergebnis hängt nicht vom
Betriebssystem ab).

- **A2 — Blockierend.** Lizenzen, Herkunft und Banne der Rust-Abhängigkeiten
  sowie bekannte Sicherheitslücken (RustSec) blockieren: Ein Verstoß und
  jede nicht ausgenommene Sicherheitslücke färben den Job rot. Hinweise
  (`unmaintained`, `unsound`) und zurückgezogene Crate-Versionen (yanked)
  bleiben sichtbare Warnungen im Protokoll und blockieren nicht. Kein
  Schritt darf seinen Fehlschlag verschlucken.
- **A12 — Erlaubte Lizenzen.** Erlaubt sind nur Lizenzen auf einer festen
  Liste (permissive Lizenzen wie MIT, Apache-2.0, BSD, ISC, Zlib, Unicode).
  GPL, AGPL und LGPL stehen nicht darauf: Ein solcher Fund ist rot und
  verlangt eine bewusste Entscheidung. MPL-2.0 ist erst nach einer
  Einzelprüfung auf die Liste gekommen. Die Liste gilt für
  Drittabhängigkeiten, nicht für die eigenen, unveröffentlichten Crates des
  Workspace. Dieselbe Liste gilt auch für die Drittlizenzen des Frontends
  (Spec 0099, A1.5).
- **A13 — Herkunft.** Rust-Abhängigkeiten kommen nur aus crates.io. Eine
  Git-Abhängigkeit oder eine andere Registry ist rot, solange sie nicht
  als begründete Ausnahme eingetragen ist (heute keine).
- **A3 — Ausnahmen.** Sicherheitshinweise werden nur an einer Stelle
  ausgenommen. Jede Ausnahme nennt das betroffene Crate, die Version, gegen
  die entschieden wurde, die Begründung und woran man erkennt, dass sie
  überholt ist. Eine stillschweigende Ausnahme gibt es nicht.
- **A4 — Überholte Ausnahmen.** Eine Ausnahme, die auf keinen aktuellen
  Befund mehr trifft (Advisory zurückgezogen, Crate nicht mehr im Baum),
  färbt den Job rot. Als Befund zählt alles, was die Prüfung ohne
  Ausnahmen meldet, Lücken **und** Hinweise. Lässt sich dieser Abgleich
  nicht durchführen, ist der Schritt rot.

## 4. Release

- **A8 — Release nur über das Gate.** Ein Release-Build (Tag `v*` oder
  manueller Start) beginnt erst, wenn die vollständigen Prüfungen aus §2
  und §3 auf genau diesem Commit grün sind. Scheitert eine davon,
  entstehen weder ein Release noch Release-Dateien. Die Prüfschritte sind
  nur an einer Stelle definiert; Pull Requests und `main` behalten ihre
  Prüfnamen.
- Das Release wird für macOS (Universal-Binary), Windows und Linux gebaut
  und als Entwurf veröffentlicht. Jedes Paket enthält die Drittlizenzen
  (Spec 0099, A2).

## 5. Grenzen

- Ein neu veröffentlichtes oder zurückgezogenes Advisory kann einen Lauf
  rot färben, ohne dass sich am Code etwas geändert hat (A2, A4). Das ist
  gewollt; die Abhilfe ist eine Aktualisierung oder eine begründete
  Änderung der Ausnahmen.
- Die wiederkehrende inhaltliche Neubewertung bestehender Ausnahmen (etwa
  bei einem Update der SSH-Bibliothek, ADR 0028) ist keine automatische
  Prüfung.
- Release-Pakete aus diesem Repository werden weder signiert noch
  notarisiert; der Release-Lauf braucht keine Secrets außer dem Zugang zum
  eigenen Repository.
- Der Start der echten App (§2a) läuft nur auf Linux. Der Start unter
  Windows und macOS (anderer Schlüsselbund, anderer Treiber) ist nicht
  abgedeckt, ebenso wenig der Pfad „Neu anfangen“, der native Dialoge
  braucht.
- Es gibt keinen zeitgesteuerten Lauf; neue Advisories fallen beim nächsten
  Lauf auf.
