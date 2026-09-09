# 0043-version-build-display-scope-choices

## Status
Akzeptiert

## Kontext

Spec 0052 (Versions- & Build-Anzeige) ließ zwei Punkte offen, an denen die
Umsetzung von einer wörtlichen Lesart der Spec abweicht:

1. **Testbarkeit von `build.rs` (Abschnitt 6)**: "Ein Build mit Git liefert
   einen Hash; ein simuliertes Build ohne Git-Zugriff liefert `"unknown"`
   statt Fehler" — ohne festzulegen, ob das ein automatisierter Test sein
   muss.
2. **Reihenfolge der "Über"-Kategorie (Abschnitt 3.2, verweist auf Spec
   0050 Abschnitt 1.1)**: Spec 0050 schlägt die Kategorien-Reihenfolge
   `… → MCP-Server → Lizenz (nur Official) → Über` vor — "Über" also nach
   einer möglichen Lizenz-Kategorie.

## Entscheidung

**1. Kein automatisierter Test für `build.rs`s Erfolgs-/Fallback-Pfad
(nur manuell verifiziert, im Commit-Text dokumentiert).**

Cargo führt Build-Scripts nie als eigenes Test-Target aus (nur `lib`/
`bin`/`tests`/`bench`, s. [Cargo-Referenz zu Targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html))
— ein `#[cfg(test)] mod tests` direkt in `build.rs` liefe unter
`cargo test` also nie, ohne dass das sichtbar wäre: stiller toter Code,
schlimmer als gar kein Test, weil er Vertrauen vortäuscht. Die
Entscheidungslogik selbst (`resolve_commit_hash`/`emit_rerun_triggers`)
ist bewusst trivial genug gehalten (reine `Option`/`Result`-Verkettung
ohne eigene Fallunterscheidung), dass eine Auslagerung in ein separat
testbares Modul nur für diese zwei Zeilen Logik unverhältnismäßig wäre.
Stattdessen automatisiert getestet (`crates/app-shell/src/version.rs`,
läuft unter `cargo test -p app-shell`): das *Format*, in dem der
eingebettete Hash weiterverwendet wird (`version_with_hash`). Das reale
Verhalten von `build.rs` selbst wurde manuell verifiziert (normaler Build
bettet den echten Hash ein, per `strings` auf dem kompilierten `.rlib`
geprüft; ein Build mit `git` versteckt aus `PATH` liefert `"unknown"`
statt eines Fehlers) — dokumentiert im Commit-Text von
`build: embed short commit hash via build.rs per spec 0052`.

**2. "Über" steht vor registrierten Sektionen (inkl. einer künftigen
privaten "Lizenz"-Kategorie), nicht danach.**

`SettingsScreen.tsx`s bestehende Struktur hängt registrierte Sektionen
(Spec 0038, `registerSettingsSection` — der Mechanismus, über den die
Official Edition ihre Lizenz-Sektion einklinkt) immer *nach* allen
eingebauten Kategorien an: `[...builtinCategories, ...registeredCategories]`.
"Über" ist eine eingebaute Kategorie (kein Registry-Kontributionspunkt),
landet also zwangsläufig vor jeder registrierten Sektion — eine
vollständige Neuordnung dieser Zusammenführungslogik nur für die exakte,
in Spec 0050 lediglich als Beispiel ("z. B.") vorgeschlagene Reihenfolge
wäre für diese reine Anzeige-Ergänzung unverhältnismäßig gewesen.

## Konsequenzen

- Ein künftiger Regressions-Fund in `build.rs`s Git-Aufruf-Logik (z. B.
  ein falsch behandelter Sonderfall) würde von `cargo test` nicht
  automatisch erkannt — nur durch erneutes manuelles Verifizieren (wie im
  Umsetzungs-Commit beschrieben) oder durch eine spätere Auslagerung der
  Logik in ein testbares Modul, falls sie einmal komplexer wird.
- Die Kategorien-Reihenfolge in den Settings ist aktuell: eingebaute
  Kategorien (zuletzt "Über") → registrierte Sektionen (z. B. eine
  künftige Lizenz-Kategorie, MCP-Server, Sitzungen & Daten). Eine private
  Official-Edition, die "Über" tatsächlich *nach* ihrer Lizenz-Kategorie
  haben möchte, bräuchte entweder eine Prioritäts-/Sortier-Erweiterung des
  `registerSettingsSection`-Mechanismus oder müsste "Über" selbst über die
  Registry statt als eingebaute Kategorie registrieren — beides nicht Teil
  dieses Schritts.
- Ein `spec-reviewer`-Review dieses Schritts bestätigte beide Abweichungen
  als bewusst und im Code kommentiert, aber ohne ADR — dieses Dokument
  schließt genau diese Lücke nach.
