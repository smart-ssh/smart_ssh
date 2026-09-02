# 0022-stable-dev-code-signature

## Status
Vorgeschlagen

## Kontext

`docs/specs/0022-credential-access-caching.md`, Abschnitt 4, skizziert die
gängige Lösung gegen wiederholte macOS-Keychain-Abfragen während
`cargo tauri dev`: ein selbstsigniertes Code-Signing-Zertifikat mit
stabiler Identität, konfiguriert über `bundle.macOS.signingIdentity`
(`tauri.conf.json`) bzw. die Umgebungsvariable `APPLE_SIGNING_IDENTITY` —
das ist auch der von `tauri-cli` selbst dokumentierte und in der
Community verbreitete Weg (s. u. a. den [macOS Code
Signing](https://v2.tauri.app/distribute/sign/macos/)-Leitfaden für
`tauri build`).

**Das funktioniert nachweislich nicht für `cargo tauri dev`.** Per
Quellcode-Analyse von `tauri-cli` 2.11.4 (`src/dev.rs` enthält keinerlei
Signier-/Bundling-Logik) und empirischer Prüfung bestätigt:
`signingIdentity`/`APPLE_SIGNING_IDENTITY` fließen ausschließlich in
`tauri_config_to_bundle_settings` (`interface/rust.rs`) ein — Code, der nur
beim tatsächlichen Bundling (`tauri build`, `tauri-bundler` + die
plattformspezifische `tauri-macos-sign`-Crate) läuft. `tauri dev` erzeugt
auf macOS **kein** `.app`-Bundle und ruft `tauri-bundler` überhaupt nicht
auf — es führt schlicht `cargo run` aus. Die einzige Signatur, die die
resultierende Binary trägt, ist die vom Rust-Linker automatisch angehängte
Ad-hoc-Signatur (Pflicht auf Apple Silicon, damit eine Binary überhaupt
startet) — inhaltsbasiert (`CDHash`), ändert sich also bei jedem Neubau.
Verifiziert mit `codesign -dv --verbose=4` gegen die tatsächlich gebaute
Dev-Binary: `flags=0x20002(adhoc,linker-signed)`, unabhängig davon, ob
`signingIdentity`/`APPLE_SIGNING_IDENTITY` gesetzt waren oder nicht.

## Entscheidung

**Eigener `--runner` statt `bundle.macOS.signingIdentity`.**
`tauri-cli` unterstützt einen konfigurierbaren `--runner`, der `cargo`
selbst ersetzt — empirisch verifiziert ruft `tauri dev` ihn exakt wie
`cargo` auf: `<runner> run [cargo-args] -- [app-args]`. Das erlaubt einen
eigenen Zwischenschritt zwischen "fertig gebaut" und "Binary startet":

1. `scripts/setup-macos-dev-signing.sh` — einmalig (idempotent): erzeugt
   ein selbstsigniertes Code-Signing-Zertifikat mit fixem Common Name
   ("Smart SSH Dev Signing") und importiert es vertrauenswürdig in den
   Login-Schlüsselbund (per `security import`/`security add-trusted-cert`,
   ganz ohne manuelle Keychain-Access-Klicks).
2. `scripts/tauri-dev-stable-signing-runner.sh` — als `--runner`
   eingehängt: baut über `cargo build --message-format=json-render-diagnostics`
   (Binary-Pfad per `grep`/`cut` aus der JSON-Ausgabe, keine
   `jq`-Abhängigkeit), signiert das Ergebnis explizit mit
   `codesign --force --sign "Smart SSH Dev Signing"` und startet danach
   erst die Binary.
3. `scripts/tauri-dev.sh` — der eigentliche Einstiegspunkt für Entwickler
   (`./scripts/tauri-dev.sh` statt `cargo tauri dev`): ruft Skript 1 auf,
   hängt Skript 2 als `--runner` ein.

Empirisch verifiziert (zwei aufeinanderfolgende, durch eine echte
Quelltextänderung ausgelöste Neubauten): `CDHash` ändert sich erwartungsgemäß
bei jedem Build, aber `codesign -d -r-` zeigt für beide Builds dieselbe
"designated requirement": `identifier "ssh-manager-app-tauri" and
certificate leaf = H"<gleicher Zertifikats-Hash>"` — genau das, woran
macOS eine "Immer erlauben"-Keychain-Freigabe bindet, nicht den `CDHash`.

**Bewusst NICHT in `tauri.conf.json` verankert.** Ein `bundle.macOS.
signingIdentity`-Eintrag dort gilt unverändert auch für `tauri build` —
und damit für den Release-Workflow auf GitHub Actions
(`.github/workflows/release.yml`), der dieses nur lokal je Entwickler-
Maschine existierende Zertifikat gar nicht kennt und dessen macOS-Build
damit brechen würde ("specified identity not found"). Die Lösung bleibt
komplett in `scripts/` gekapselt und wirkt nur, wenn ein Entwickler
explizit `./scripts/tauri-dev.sh` aufruft.

## Konsequenzen

**Positiv:**
- Löst das eigentliche Problem (bestätigt durch Vorher/Nachher-Vergleich
  der `codesign`-Ausgabe), nicht nur eine Konfiguration, die für `tauri
  build`, aber nicht für `tauri dev` gegriffen hätte.
- Kein Risiko für den Release-Build: `tauri.conf.json` bleibt unverändert,
  nichts davon wirkt außerhalb von `scripts/tauri-dev.sh`.
- Vollständig automatisiert (Zertifikat-Erzeugung, Vertrauenseinstellung,
  Signieren) — kein manueller Keychain-Access-Schritt.

**Negativ / Trade-off:**
- Ein zusätzlicher, projektspezifischer Wrapper statt eines reinen
  `tauri.conf.json`-Eintrags — wer weiterhin direkt `cargo tauri dev`
  aufruft (ohne den Wrapper), bekommt weiterhin die instabile
  Linker-Ad-hoc-Signatur und damit wiederholte Keychain-Abfragen. Bewusst
  in Kauf genommen: die Alternative (die Signatur automatisch bei jedem
  reinen `cargo tauri dev` erzwingen) hätte einen Eingriff in
  `tauri.conf.json` verlangt, der wie oben beschrieben den Release-Build
  gefährdet hätte.
- Die Shell-Skripte sind auf Bash-3.2-Kompatibilität geachtet (macOS'
  vorinstallierte Bash, letzte GPLv2-Version) — insbesondere das
  `"${arr[@]+"${arr[@]}"}"`-Muster für potenziell leere Arrays unter
  `set -u`, das unter Bash 3.2 sonst fälschlich "unbound variable" wirft.
  Ein subtiler Stolperstein, falls die Skripte künftig erweitert werden.
- Nur für macOS relevant (`scripts/tauri-dev.sh` fällt auf anderen
  Plattformen auf ein normales `cargo tauri dev` zurück) — Windows/Linux
  kennen dieses Ad-hoc-Signatur-Problem nicht.
