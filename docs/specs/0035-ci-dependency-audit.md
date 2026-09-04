# Spec: CI-Härtung — Dependency-Audit & Lizenz-Compliance

Status: Entwurf
Modul: Erweiterung der CI-Pipeline (`.github/workflows/ci.yml`, Spec 0001)
Abhängigkeiten: keine fachliche, reine Tooling-Ergänzung

## 1. Ziel

Zwei bisher fehlende automatisierte Prüfungen werden Teil der CI-Pipeline:

- **`cargo audit`**: bekannte Sicherheitslücken in Abhängigkeiten (RustSec-
  Advisory-Datenbank) gegen `Cargo.lock`.
- **`cargo deny`**: Lizenz-Compliance (setzt die in D1 des
  Architektur-Briefs verbindlich festgelegte Regel technisch durch, nicht
  nur als Vorsatz) sowie Herkunfts-Prüfung der Abhängigkeiten
  (nur crates.io, keine unbekannten Registries/Git-Quellen ohne bewusste
  Ausnahme).

## 2. `cargo-deny`-Konfiguration (`deny.toml`)

- **Lizenzen**: Default-Verhalten `deny`, explizite Allow-Liste gemäß D1
  (MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, sowie
  Unicode-DFS-2016; MPL-2.0 **nur nach Einzelprüfung** — MPL ist
  file-level Copyleft, in der Praxis für dieses Projekt meist
  unproblematisch, aber laut D1 ausdrücklich nicht pauschal zu erlauben).
  GPL/AGPL/LGPL **nicht** in der Allow-Liste — jeder Fund erzwingt eine
  bewusste Entscheidung statt automatisch durchzurutschen.
- **Quellen**: nur `crates.io` als erlaubte Registry, keine Git-Abhängigkeiten
  ohne explizite, kommentierte Ausnahme in `deny.toml`.
- **Advisories**: `cargo deny` kann das ebenfalls prüfen, aber `cargo audit`
  bleibt das primäre Werkzeug dafür (Abschnitt 3) — keine doppelte
  Konfiguration derselben Prüfung an zwei Stellen.

## 3. `cargo-audit`-Konfiguration

- Läuft gegen `Cargo.lock` über die gesamte Workspace.
- **Ausnahmen** (`audit.toml`, `ignore = [...]`) nur mit Begründungs-
  Kommentar direkt daneben — nie eine stillschweigend ignorierte Advisory.

## 4. CI-Integration

Ein neuer, eigenständiger Job in der bestehenden GitHub-Actions-Pipeline
(Spec 0001) — läuft auf `ubuntu-latest` (Abhängigkeits-/Lizenzprüfung ist
nicht plattformspezifisch, ein einzelner Lauf reicht, kein Matrix-Bedarf wie
bei den Build-Jobs).

**Verhältnis zu `community.yml` (Spec 0038)**: Spec 0038 nimmt
`cargo deny check licenses` als Pflichtschritt in den dortigen
Community-Workflow auf. Das ist **kein zweiter, konkurrierender
Mechanismus** — dieselbe `deny.toml`, dieselben Regeln. Sobald
`community.yml` existiert und der Bestand gemäß Abschnitt 5 bereinigt ist,
kann der hier beschriebene eigenständige Job darin aufgehen, statt beide
parallel zu pflegen. Bis dahin ist der hier beschriebene Job der
maßgebliche.

## 5. Fehlschlag-Politik — zweistufiges Vorgehen

Da unklar ist, wie der aktuelle Bestand an Abhängigkeiten heute schon
dasteht, **zuerst im Berichtsmodus einführen**: Job läuft, Ergebnisse werden
sichtbar (z. B. als CI-Ausgabe/Artefakt), aber der Build schlägt **noch
nicht** fehl. Erst nachdem der aktuelle Bestand bereinigt bzw. bewusst mit
begründeten Ausnahmen versehen ist (Abschnitt 2/3), wird der Job auf
**blockierend** umgestellt (Fehlschlag bei jeder neuen, nicht ignorierten
Advisory oder nicht erlaubten Lizenz). Reihenfolge ist wichtig — ein sofort
blockierender Job auf einem ungeprüften Bestand würde vermutlich sofort
die gesamte Pipeline rot färben, ohne dass jemand die Fundstellen einzeln
bewertet hätte.

## 6. Offene Punkte

- Automatisierte, geplante (nicht nur push-getriggerte) Läufe — z. B.
  wöchentlich, damit neu bekannt gewordene Advisories auch ohne neuen Commit
  auffallen — sinnvolle Ergänzung, aber nicht zwingend Teil dieses ersten
  Schritts.
