# ADR 0148 — Mobile-Build-Struktur (Android)

Status: angenommen · Issue: #675 · Spec 0114

## Kontext

Die App war ein reines Desktop-Binary. Tauri-Mobile lädt die App als Bibliothek. Mehrere Abhängigkeiten haben unter Android kein Backend (`rfd`, `clipboard-rs`, `tauri-plugin-single-instance`, `tauri-plugin-decoration`, Ordnerauswahl in `tauri-plugin-dialog`), und `directories` liefert dort ohne `HOME` nichts.

## Entscheidungen

1. **Bibliothek plus Binary.** Die App-Crate bekommt ein `[lib]` (`staticlib`, `cdylib`, `rlib`) mit `#[cfg_attr(mobile, tauri::mobile_entry_point)] run()`; `main.rs` ruft dieselbe Funktion. Der Desktop-Pfad bleibt unverändert.
2. **Gating mit `target_os = "android"`**, nicht mit `cfg(desktop)`: Die Crates unter `crates/` haben kein Build-Skript von `tauri-build`, das `desktop`/`mobile` setzt. Die betroffenen Abhängigkeiten stehen unter `[target.'cfg(not(target_os = "android"))'.dependencies]`, damit sie für Android nicht gebaut werden. Capabilities: `default.json` nur für Linux/macOS/Windows, `mobile.json` ohne Fensterrahmen-Rechte für Android.
3. **Ersatz statt Weglassen an der Grenze.** Startdialoge loggen und beenden (Rückfrage = Abbrechen); die Zwischenablage-Datei-Funktionen liefern einen Fehler; `pick_folder` wird abgelehnt; der Pseudo-Server wird an der Listen-Grenze nicht angeboten (nie in der Sicherheitslogik).
4. **Pfade.** Auf Android liefert ein fester, rein berechneter Pfad `/data/data/<Bezeichner>/files` (Daten, `logs`, `downloads`) bzw. `.../cache`. Grund für die Abweichung von Tauris Pfad-API: Datenbank-Sperre und -Öffnung laufen vor dem Aufbau der Tauri-App, ein `AppHandle` existiert dann noch nicht. Der Pfad entspricht `getFilesDir()`/`getCacheDir()` des Hauptnutzers; ein Test sichert, dass der Bezeichner dem in `tauri.conf.json` entspricht. Der Bezeichner `com.smartssh.desktop` bleibt (Android akzeptiert ihn). Eine einbettende Anwendung mit anderer Application-ID setzt sie einmalig beim Start per `set_android_app_id` (vor dem ersten Pfad-Lookup; danach oder bei abweichendem zweiten Wert wird abgelehnt), damit sich die Ordner im Prozess nie ändern; ohne Aufruf gilt der Standard.
5. **Plattform.** `get_platform` liefert `android`; `create_overlay_titlebar` meldet `native`. Das Frontend erzwingt das schmale Layout über ein Attribut am Wurzelelement.

## Offen (muss auf Gerät/Emulator geprüft werden)

- **TLS:** `reqwest` mit `rustls` nutzt `rustls-platform-verifier`, das unter Android eine JNI-Initialisierung samt Kotlin-Komponente braucht. Entscheidung: unter Android (`cfg(target_os = "android")`) bauen `ai-providers` und `speech` ihre HTTP-Clients mit einer vorkonfigurierten rustls-Konfiguration auf Basis der eingebauten `webpki-roots`; Desktop bleibt unverändert beim Plattform-Verifier. Begründung: kein Kotlin-/JNI-Code, auf dem Host ohne Gerät übersetzbar und testbar. Nachteil: Wurzelzertifikate, die der Nutzer im Android-Zertifikatsspeicher installiert hat (z. B. Firmen-CA), werden nicht berücksichtigt; ein späteres Issue kann den Plattform-Verifier nachrüsten.
- **Build-Ausgabe von `tauri android init`** (`gen/android`) ist eingecheckt. Der Android-`ndk-context` kommt ab tao 0.37 von tao selbst (Issue #820); die frühere Umgehung (Issue #795) ist entfernt, weil sie doppelt initialisiert und beim Start paniert. Der Test `apps/smart-ssh-community/tests/android_ndk_context.rs` koppelt das an die tao-Version im Lockfile.
- **Krypto-Provider (Issue #935):** Die vorkonfigurierte Konfiguration übergibt den Provider `aws-lc-rs` ausdrücklich (gemeinsamer Helfer in der Crate `tls-config`, von `ai-providers` und `speech` genutzt), statt den Prozess-Standard von rustls zu verwenden; `app_shell::run` installiert `aws-lc-rs` zusätzlich einmalig als Prozess-Standard. Grund: Mit `ring` (über `sqlx`/`tls-rustls`) und `aws-lc-rs` gleichzeitig kann rustls keinen Standard wählen und paniert. Das unnötige `tls-rustls`-Feature von `sqlx` ist entfernt. Zertifikatsprüfung unverändert.
