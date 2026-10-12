# ADR 0130 — Die echte App in der CI starten (tauri-driver, Rust-Tests)

Status: akzeptiert
Betrifft: Issue #235, Spec 0090 (§2a), Spec 0101, Issue #166, #234

## Kontext

Kein bestehender Test startet die echte App: Die Playwright-Tests (ADR 0120)
laufen gegen ein nachgebautes Backend, die Rust-Tests bauen den `AppState`
ohne Tauri. Issue #234 (Zustand nie an Tauri übergeben) blieb deshalb
unbemerkt.

## Entscheidung

1. **Treiber und Sprache.** `tauri-driver` (WebDriver) mit `thirtyfour` in
   Rust, als Dev-Abhängigkeit einer eigenen, reinen Test-Crate
   `crates/real-app-tests`. Weder eine Produktiv-Crate noch das Frontend
   bekommt eine neue Abhängigkeit. Ohne das `manager`-Feature, damit keine
   Treiber zur Laufzeit geladen werden. Alles steht unter
   `[dev-dependencies]` und die Crate hat kein Bibliotheksziel: `thirtyfour`
   zieht `reqwest`, und die Grenzprüfung für HTTP-Clients
   (`network_boundary.rs`, Produktionsgraph) darf das nicht sehen. Es ist
   keine ausgehende Verbindung der App.
2. **Ausführung.** Die Tests sind `#[ignore]`; `cargo test --workspace`
   bleibt ohne Treiber grün. Der CI-Job ruft sie mit `--ignored` und
   `--test-threads=1` auf (Schlüsselbund und Einzelinstanz-Name sind
   geteilt). Ein Mutex in der Test-Datei erzwingt die Reihenfolge auch ohne
   das Flag.
3. **Fixtures.** Keine neue eingecheckte Binärdatei. Die Aktualisierung
   nutzt die vorhandene, vom Release 0.5.2 selbst geschriebene Datei
   `v0.5.2.sqlite3` (ADR 0105) und legt deren Zugangsdaten über
   `credentials-keyring` in den Test-Schlüsselbund. Der Passwort-Modus
   entsteht über `connect_encrypted` und `set_up_master_password`, also
   über dieselben öffentlichen Funktionen wie in der App. Das Datenverzeichnis
   steuert `SMART_SSH_DATA_DIR`.
4. **Kein Test-Zugang in der App.** Weder die ausgelieferte App noch die
   sicherheitsrelevanten Module bekommen einen Testhaken; die Tests nutzen
   nur das vorhandene Kommando-Interface (`window.__TAURI_INTERNALS__`)
   und das Fenster.
5. **Frischer Start prüft „keine Server“.** Das frische Datenverzeichnis hat
   außer dem lokalen Pseudo-Server keinen Server; erwartet wird eine
   beantwortete, leere Liste und die angelegte Datenbankdatei.

## Konsequenzen

- Die Test-Crate wird auf allen drei Betriebssystemen mitgebaut
  (`cargo test --workspace`), aber nur unter Linux ausgeführt.
- Die App muss mit eingebetteten Frontend-Dateien gebaut sein
  (`npm run build`, dann `cargo build -p smart-ssh-community --features
  tauri/custom-protocol`), sonst lädt sie die Entwicklungs-URL.
- Windows und macOS sind nicht abgedeckt (Spec 0090 §5).
