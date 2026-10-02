//! App-Shell (Spec 0038): Library, die den bisherigen `app-tauri`-Aufbau
//! (Tauri-Commands, `AppState`-Aufbau, Event-Handling — alles seit Spec
//! 0007) unverändert in der Substanz bereitstellt, jetzt aber
//! Edition-parametrisiert über [`Wiring`] statt fest verdrahtet. Ein
//! konkretes Binary (aktuell nur `apps/smart-ssh-community`, künftig auch
//! ein privates `Official`-Pendant, s. Spec 0038 Abschnitt 3) unterscheidet
//! sich nur im übergebenen `Wiring` und ruft [`run`] auf.
//!
//! Weiterhin: nur Tauri-Commands, die Core-APIs aufrufen und DTOs
//! zurückgeben, keine fachliche Logik hier (Spec 0007, Abschnitt 3) — diese
//! Abgrenzung gilt unverändert, nur die Crate-Grenze hat sich verschoben.

mod chat_retention;
mod commands;
mod elevated_sftp;
/// Spec 0084, §4: der Newtype, der die `EventEmitter`-Impl für
/// `tauri::AppHandle` trägt (s. dortiger Moduldoc-Kommentar).
mod event_emitter;
mod first_run_notice;
mod local_server;
mod mcp_backend;
mod mcp_settings;
mod risk_second_opinion;
/// Spec 0075, §7.3: den bestätigten Importplan ausführen — der einzige
/// Schritt, in dem überhaupt eine Schlüsseldatei geöffnet wird (§5.1).
mod ssh_config_apply;
/// Spec 0075, §7.4: Export nach `ssh_config` — Dateidialog,
/// `~/.ssh/config`-Ablehnung, Schreiben. Die Abbildung selbst liegt in
/// `ssh_manager_core::profiles::ssh_config::export`.
mod ssh_config_export;
mod startup_dialog;
/// Spec 0101, A3/A5: die nativen Startdialoge zu den Fällen D1–D4 —
/// Zuordnung von Fall zu Text und Knöpfen, ohne eigene Logik.
mod startup_prompt;
#[cfg(test)]
mod test_support;
mod wiring;

pub use wiring::{Edition, Wiring};

use std::sync::Arc;

use credentials_keyring::KeyringCredentialStore;
use persistence_sqlite::default_db_path;

use app_logic::confirmation::ConfirmationRegistry;
use app_logic::host_key_store::FileHostKeyStore;
use app_logic::session::SessionManager;
use app_logic::state::AppState;

/// Baut den `AppState` einmalig beim App-Start auf. Synchron nach außen
/// (`run()` wird von `main.rs` ohne `#[tokio::main]` aufgerufen, wie im
/// Standard-Tauri-Bootstrap üblich) — `tauri::async_runtime::block_on`
/// überbrückt den einen async `SqliteProfileStore::connect`-Aufruf beim
/// Start; danach läuft alles über Tauris eigene, bereits laufende
/// Async-Runtime (jedes `#[tauri::command]` ist selbst `async fn`).
fn build_app_state(
    wiring: &Wiring,
    log_guard: tracing_appender::non_blocking::WorkerGuard,
) -> (AppState, tracing_appender::non_blocking::WorkerGuard) {
    // Spec 0071, A11a/A11b: EINE Sprachwahl für ALLE Startdialoge dieses
    // Programmlaufs — hier, an der einzigen Stelle, die tatsächlich die
    // Umgebung liest. Die Entscheidungslogik selbst ist rein und liegt in
    // `startup_error_messages` (A11c). Würde jeder Dialog seine Sprache
    // selbst bestimmen, zeigte ein englischsprachiges System bei einem
    // DB-Fehler Deutsch und bei einem Schlüsselbund-Fehler Englisch.
    let lc_all = std::env::var("LC_ALL").ok();
    let lc_messages = std::env::var("LC_MESSAGES").ok();
    let lang = std::env::var("LANG").ok();
    let language = app_logic::startup_error_messages::startup_language(
        app_logic::startup_error_messages::preferred_locale_value(
            lc_all.as_deref(),
            lc_messages.as_deref(),
            lang.as_deref(),
        ),
    );

    let db_path = default_db_path();

    // Spec 0071, A3/A16: Das Session-Bus-Indiz und der Schlüsselbund-Zustand
    // werden hier EINMAL pro Programmlauf ermittelt und danach im `AppState`
    // weitergereicht — kein Kommando probiert den Schlüsselbund zusätzlich
    // ab, um den Zustand zu erfahren (insbesondere nicht `list_servers`, das
    // `ServerDto::from_server` pro Server aufruft).
    //
    // Nur diese Stelle liest die Umgebung; die Regel selbst ist rein und
    // liegt in `credentials_keyring::session_bus_present` (A3). Der Wert von
    // `DBUS_SESSION_BUS_ADDRESS` wird dabei zu einem `bool` verdichtet und
    // nirgends weitergereicht — ein Steuerzeichen darin kann deshalb keinen
    // Dialogtext fortsetzen (X1).
    //
    // **Seit Spec 0101 vor dem Öffnen der Datenbank** (A3): Der
    // Schlüsselbund-Zustand ist eine Eingabe der Entscheidungstabelle, nicht
    // mehr eine Nachbemerkung zu einer schon offenen Datenbank.
    let dbus_address = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok();
    let xdg_runtime_bus_exists = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .is_some_and(|dir| std::path::Path::new(&dir).join("bus").exists());
    let keychain = credentials_keyring::probe_keychain_availability(
        std::env::consts::OS,
        credentials_keyring::session_bus_present(dbus_address.as_deref(), xdg_runtime_bus_exists),
    );
    // spec-reviewer-Fund zur Klarstellung Q-BL-0031-01: §6.4 M1 verlangt
    // "Log enthält den vollen Grund". Der klassifizierte Zustand oben ist
    // dafür zu grob — die eigentliche `store_status()`-Fehlerkette existiert
    // nur hier. Sie geht ausschließlich ins Log, nie in einen Dialog, ein
    // DTO oder das Diagnosepaket (I1/X2; die Zeile steht bewusst NICHT in
    // `diagnostics::SAFE_LOG_MESSAGES` und fliegt damit aus dem exportierten
    // Paket).
    if let Some(chain) = credentials_keyring::store_status_cause_chain_for_log() {
        tracing::warn!(cause_chain = %chain, "OS keychain store initialisation failed");
    }
    tracing::info!(?keychain, "probed OS keychain availability");

    let credential_store = KeyringCredentialStore::new();

    // Spec 0101, A3/A5/A6: Der Start entscheidet nach Dateizustand und
    // Schlüsselzustand, **bevor** eine Migration läuft — und fasst in keinem
    // Dialogfall etwas an, solange der Nutzer nicht gewählt hat. Die
    // Entscheidung selbst liegt Tauri-frei in `app_logic::database_startup`
    // (dort auch die Begründung, warum); hier wird nur der native Dialog
    // beigesteuert.
    //
    // **Ersetzt den vorherigen `connect` + `resolve_or_generate_key`.** Die
    // alte Reihenfolge öffnete und migrierte die Datenbank zuerst und holte
    // den Schlüssel danach — mit SQLCipher geht das nicht mehr, denn ohne
    // Schlüssel ist schon das Öffnen nicht möglich (A4: nie „out of memory"
    // aus dem Migrationslauf für einen Schlüssel-Fall).
    tracing::info!(data_path = %db_path.display(), "opening the encrypted SQLite database");
    let prompt = crate::startup_prompt::NativeStartupPrompt {
        db_path: db_path.clone(),
        language,
        keychain,
    };
    let opened =
        match tauri::async_runtime::block_on(app_logic::database_startup::open_or_prepare_database(
            &db_path,
            &credential_store,
            keychain,
            &prompt,
        )) {
            Ok(opened) => opened,
            // Der Nutzer hat „Beenden" gewählt. Es ist bereits alles gesagt —
            // ein zweiter Dialog wäre nur Lärm. Rückgabewert 0, weil ein
            // bewusstes Beenden kein Fehler ist (anders als bei
            // `show_fatal_error_and_exit`).
            Err(app_logic::database_startup::StartupAbort::UserQuit) => {
                tracing::info!("startup aborted by the user");
                // s. Kommentar zum `drop(log_guard)` unten — `process::exit`
                // führt keine Destruktoren aus, der Log-Puffer würde sonst
                // verloren gehen.
                drop(log_guard);
                std::process::exit(0);
            }
            Err(app_logic::database_startup::StartupAbort::Fatal { kind, detail }) => {
                let log_dir = app_logic::logging::default_log_dir();
                let text = app_logic::startup_error_messages::db_connect_failure_text(
                    &kind, &db_path, &log_dir, language,
                );
                // `detail` **nur** ins Log: Er kann einen Bibliothekstext
                // enthalten, der Dialog nennt stattdessen Ursache, Datenpfad und
                // nächsten Schritt (Spec 0059, Invarianten).
                tracing::error!(detail, ?kind, "fatal: database startup failed");
                // spec-reviewer-Fund: `std::process::exit` in `show_fatal_error_
                // and_exit` führt keine Destruktoren aus — ohne dieses explizite
                // `drop` würde der `WorkerGuard` (der den nicht-blockierenden
                // Log-Writer beim Drop synchron flusht, s. `logging::init_
                // logging`-Doc-Kommentar) nie laufen, und ausgerechnet die
                // `tracing::error!`-Zeile zum fatalen Fehler könnte im Puffer
                // verloren gehen.
                drop(log_guard);
                crate::startup_dialog::show_fatal_error_and_exit(&text.title, &text.message);
            }
        };
    tracing::info!("SQLite database connected");
    let profile_store = opened.store;
    let chat_content_key = opened.root_key;
    let ai_provider_store = profile_store.ai_provider_store();
    let policy_store = profile_store.policy_store();

    // Spec 0036/0040/0057: derselbe Cipher (und damit derselbe Schlüssel)
    // für alle drei Stores — kein weiterer Verschlüsselungsmechanismus für
    // `prompt_history`/`ledger`.
    //
    // **Seit Spec 0101 nie mehr `None`** (E11, A3): Die Datenbank ist
    // überhaupt nur offen, wenn K vorlag — der frühere Zustand „App läuft,
    // aber Chat, Historie und Ledger sind abgeschaltet" (Spec 0040,
    // Abschnitt 7) kann auf diesem Weg nicht mehr entstehen. Ohne K endet
    // der Start in D1/D2/D3, nicht in einer halb benutzbaren App.
    let chat_content_cipher: Arc<dyn ssh_manager_core::crypto::ContentCipher> = Arc::new(
        ssh_manager_core::crypto::ChaCha20Poly1305Cipher::new(&chat_content_key),
    );
    let (prompt_history_store, chat_session_store, ledger_store) = (
        Some(profile_store.prompt_history_store(chat_content_cipher.clone())),
        Some(profile_store.chat_session_store(chat_content_cipher.clone())),
        Some(profile_store.ledger_store(chat_content_cipher)),
    );

    // Host-Keys leben bewusst neben (nicht in) der SQLite-Datenbank — s.
    // `app_logic::host_key_store`-Modul-Kommentar zur Begründung (der
    // `HostKeyStore`-Trait ist absichtlich synchron, `sqlx` ist es nicht).
    let host_key_path = db_path
        .parent()
        .expect("db_path hat immer ein Elternverzeichnis (s. default_db_path)")
        .join("host_keys.json");
    tracing::info!(path = %host_key_path.display(), "loading host-key store");
    // Spec 0059: kein eigener, benannter Fall, aber auf demselben
    // Datenverzeichnis wie Fall 4 und derselben Fehlerklasse (Zugriffs-/
    // Korruptionsproblem) — mit demselben Mechanismus geschlossen, statt
    // eines bekannten `.expect(...)` direkt neben den vier behobenen
    // Fällen unangetastet zu lassen (s. `startup_error_messages::
    // host_key_store_failure_text`-Doc-Kommentar).
    let host_key_store = match FileHostKeyStore::load(host_key_path.clone()) {
        Ok(store) => store,
        Err(err) => {
            let text = app_logic::startup_error_messages::host_key_store_failure_text(
                &host_key_path,
                language,
            );
            tracing::error!(error = %err, "fatal: host-key store failed to load");
            // s. Kommentar bei der DB-Verbindung oben — dieselbe explizite
            // Log-Flush-Notwendigkeit vor `process::exit`.
            drop(log_guard);
            crate::startup_dialog::show_fatal_error_and_exit(&text.title, &text.message);
        }
    };
    tracing::info!("host-key store loaded");

    let app_state = AppState {
        sessions: SessionManager::new(),
        profile_store: Arc::new(profile_store),
        credential_store: Arc::new(credential_store),
        // Spec 0076, §4.2: zustandslos — sie hält nichts fest, weil bei
        // jedem Verbindungsaufbau neu gelesen wird (E-5, §4.3).
        key_file_reader: Arc::new(app_logic::key_files::OsKeyFileReader::new()),
        keychain,
        ai_provider_store: Arc::new(ai_provider_store),
        host_key_store: Arc::new(host_key_store),
        policy_store,
        prompt_history_store,
        chat_session_store,
        ledger_store,
        // Spec 0038, Abschnitt 2: aus dem übergebenen `Wiring` gelesen statt
        // hier fest verdrahtet (s. `Wiring::community`-Doc-Kommentar zum
        // Scope dieses Refactorings).
        entitlements: wiring.entitlements.clone(),
        pending_host_key_confirmations: ConfirmationRegistry::new(),
        pending_action_confirmations: ConfirmationRegistry::new(),
        running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
        mcp: app_logic::state::McpState::default(),
        rate_limit_registry: ai_providers::RateLimitRegistry::new(),
        // Spec 0075, §5.1: leer, bis eine Vorschau gelaufen ist.
        pending_ssh_config_import: std::sync::Mutex::new(None),
    };
    (app_state, log_guard)
}

/// Startet die App mit der übergebenen [`Wiring`]/[`tauri::Context`].
///
/// Der `Context` wird bewusst vom Aufrufer (`main.rs` des jeweiligen
/// Binaries, z. B. `apps/smart-ssh-community`) übergeben statt hier per
/// `tauri::generate_context!()` erzeugt: das Makro liest `tauri.conf.json`
/// relativ zum `CARGO_MANIFEST_DIR` der Aufrufstelle zur Kompilierzeit —
/// stünde der Makroaufruf hier in `app-shell`, müssten `tauri.conf.json`/
/// Icons/Frontend dieser Library-Crate zugeordnet sein, obwohl `app-shell`
/// künftig mehrere Binaries (Community/Official) mit je eigener Config
/// bedienen soll (Spec 0038, Abschnitt 3).
pub fn run(wiring: Wiring, context: tauri::Context<tauri::Wry>) {
    // Spec 0016, Abschnitt 2/3: so früh wie möglich, damit auch Fehler beim
    // App-Setup selbst (z. B. `build_app_state()`s DB-Verbindungsaufbau)
    // bereits strukturiert geloggt würden. `_log_guard` muss über die
    // gesamte App-Laufzeit am Leben bleiben (s. `app_logic::logging::
    // init_logging`-Doc-Kommentar) — `run()` unten blockiert bis zum
    // Beenden der App, danach ist ein finaler Flush ohnehin irrelevant.
    let _log_guard = app_logic::logging::init_logging();

    // Spec-Reviewer-Fund (Spec 0047, Review dieses Schritts): muss VOR dem
    // ersten Aufruf installiert sein, der selbst panicken kann —
    // `default_db_path()` unten hat ein `.expect(...)` (kein Home-/
    // XDG-Verzeichnis auflösbar). Vorher stand der Hook erst nach diesem
    // Aufruf, sodass genau dieser frühe Panic wieder spurlos nur nach
    // stderr gegangen wäre — derselbe "spurlos verschwunden"-Fall, den
    // dieser Hook eigentlich schließen soll. (Noch früher, vor
    // `init_logging()` selbst, brächte nichts: ohne registrierten
    // `tracing`-Subscriber ist `tracing::error!` ein stiller No-op — ein
    // Panic in der Log-Verzeichnis-Auflösung selbst lässt sich prinzipiell
    // nicht in die Logdatei schreiben, die dieser Aufruf gerade erst
    // anlegen soll.) `set_hook` ERSETZT den Standard-Hook, deshalb wird er
    // hier explizit mit aufgerufen (nicht nur geloggt) — beim Starten aus
    // einem Terminal (Entwicklung) bleibt die gewohnte Konsolenausgabe
    // erhalten.
    let default_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(panic = %info, "app panicked");
        default_panic_hook(info);
    }));

    // Spec 0047, Fund B1: erste Logzeile enthält App-Version, OS/Plattform
    // und den aufgelösten Datenpfad — genau die drei Angaben, die ein
    // Tester beim Melden eines "geht nicht" sonst manuell mitteilen
    // müsste. `context` (mit `package_info()`) liegt bereits vor, ohne
    // dass dafür irgendetwas geöffnet/verbunden werden muss.
    //
    // Spec 0052, Abschnitt 3.1: Version allein identifiziert einen Build
    // nicht eindeutig (mehrere Builds können dieselbe Version tragen) —
    // `commit_hash` (strukturiertes Feld, für ein grep/Log-Tool-Filtering)
    // ergänzt um `version_display` im überall geteilten Anzeigeformat
    // (`app_logic::version::version_with_hash`, s. dortiger Doc-Kommentar),
    // damit ein an einen Bug-Report angehängtes Log auch beim bloßen
    // Überfliegen sofort die exakte Build-Kennung zeigt.
    let db_path = default_db_path();
    let version = context.package_info().version.to_string();
    tracing::info!(
        version = %version,
        commit_hash = app_logic::version::BUILD_COMMIT_HASH,
        version_display = %app_logic::version::version_with_hash(&version),
        build_type = app_logic::version::BuildType::current().as_str(),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        data_path = %db_path.display(),
        "Smart SSH startet"
    );

    // Spec 0052, Abschnitt 3.2: `Edition` (`Copy`) separat gesichert, bevor
    // `wiring.plugins` unten per Wert herausgezogen wird (ein partieller
    // Move einzelner Felder ist erlaubt) — der Über-Dialog-Command
    // `get_app_info` braucht sie als `State<Edition>`, `Wiring` selbst
    // wird nirgends als Tauri-`State` verwaltet.
    let edition = wiring.edition;
    // spec-reviewer-Fund (Spec 0059): `_log_guard` wird hier zur
    // Weiterreichung an `build_app_state` per Wert übergeben (das
    // ansonsten fatale `.expect()`-freie Verhalten dort kann den Guard bei
    // einem der drei fatalen Fehlerfälle explizit droppen, um den
    // Log-Writer VOR `std::process::exit` blockierend zu flushen — s.
    // Kommentare in `build_app_state`) und danach für die restliche
    // App-Laufzeit zurückgegeben.
    let (app_state, _log_guard) = build_app_state(&wiring, _log_guard);
    let plugins = wiring.plugins;

    let builder = tauri::Builder::default();
    let builder = plugins
        .into_iter()
        .fold(builder, |builder, plugin| plugin(builder));

    builder
        // Für die Key-/Zertifikat-Datei-Auswahl im Server-Formular (Spec
        // 0008, Abschnitt 6): der native Dialog läuft im Backend
        // (`commands::read_credential_file`, Spec 0013 SEC-06 — unabhängiger
        // Review-Pass ersetzte hier den vorherigen `plugin-fs`-basierten
        // Ansatz, bei dem das Webview die Datei clientseitig gelesen hätte),
        // der Pfad selbst wird nie gespeichert (Spec 0008 Abschnitt 8). Kein
        // `tauri_plugin_fs` mehr registriert — nichts nutzt es noch, und die
        // `capabilities/default.json` gewährt ohnehin keine `fs:*`-Rechte;
        // ein ungenutztes, geladenes Plugin ist unnötige Angriffsfläche.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Spec 0024, Abschnitt 4: Speicherort für die gewählte UI-Sprache
        // (und künftige reine UI-Einstellungen wie ein Theme) — bewusst kein
        // sekundärer SQLite-Migrationspfad für eine einzelne, nicht
        // sicherheitsrelevante Einstellung.
        .plugin(tauri_plugin_store::Builder::new().build())
        // Spec 0024, Abschnitt 4: liefert die System-Locale für die
        // Sprachermittlung beim ersten Start (`frontend/src/i18n.ts`).
        .plugin(tauri_plugin_os::init())
        // Entscheidung für tauri-plugin-decoration statt tauri-plugin-decorum:
        // tauri-plugin-decorum (v0.1.6) wird nicht mehr aktiv gepflegt und wirft Build-Fehler
        // bei modernen Rust-Toolchains/macOS-SDKs. tauri-plugin-decoration (v3.0.5) ist aktiv
        // gepflegt, unterstützt Tauri v2.10+ und verwaltet native macOS-Ampel-Insets sowie Windows Snap Layouts.
        .plugin(tauri_plugin_decoration::init())
        // Spec 0028, Abschnitt 9a: native Toast-Benachrichtigung bei einer
        // wartenden MCP-Bestätigung (s. `crate::mcp_backend`).
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            use tauri::Manager;
            use tauri_plugin_decoration::WebviewWindowExt;

            // Spec 0047, Fund B1: letzter Startschritt — steht dieser
            // Eintrag im Log, ist die App bis zum Fenster durchgestartet;
            // fehlt er, bricht der Start irgendwo davor (DB/Keychain/
            // Plugin-Setup) ab, was die Schritte oben eingrenzen.
            tracing::info!(
                window_found = app.get_webview_window("main").is_some(),
                "app setup complete, creating main window"
            );

            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = window_clone.activate_decoration().await;
                    #[cfg(target_os = "macos")]
                    {
                        // Spec 0014 Abschnitt 3 & 6: macOS Ampel-Inset (Startwert 12.0, 16.0)
                        let _ = window_clone.set_traffic_lights_inset(12.0, 16.0).await;
                    }
                });
            }

            // Spec 0028, Abschnitt 9: ein beim letzten Beenden aktivierter
            // MCP-Server bleibt über einen Neustart hinweg aktiv, ohne
            // manuelles erneutes Anschalten.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                crate::mcp_settings::autostart_if_enabled(&handle).await;
            });

            // Spec 0034, Abschnitt 5: Aufbewahrungs-Aufräum-Job beim
            // App-Start — No-op, solange keine Aufbewahrungsdauer
            // konfiguriert ist (Default).
            let handle_for_retention = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = handle_for_retention.state::<AppState>();
                crate::chat_retention::cleanup_old_chat_sessions_on_startup(
                    &handle_for_retention,
                    &state,
                )
                .await;
            });

            // Spec 0040, Abschnitt 3: einmalige, idempotente Migration
            // bestehender Klartext-Zeilen in `prompt_history` — No-op,
            // sobald alle Zeilen bereits verschlüsselt sind (jeder Start
            // danach prüft erneut, findet aber nichts mehr zu tun).
            let handle_for_prompt_history_migration = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = handle_for_prompt_history_migration.state::<AppState>();
                // Spec 0040, Abschnitt 7: `None`, wenn der Verschlüsselungs-
                // schlüssel beim Start nicht aufgelöst werden konnte (s.
                // `build_app_state`) — dann gibt es nichts zu migrieren.
                let Some(store) = &state.prompt_history_store else {
                    return;
                };
                match store.migrate_legacy_plaintext_content().await {
                    Ok(count) if count > 0 => {
                        tracing::info!(count, "legacy plaintext prompt_history rows encrypted");
                    }
                    Ok(_) => {}
                    Err(err) => {
                        tracing::warn!(error = %err, "prompt_history encryption migration failed");
                    }
                }
            });

            // Spec 0038, Abschnitt 4: hält das Frontend über
            // `entitlements:changed` aktuell, sobald `EntitlementProvider::
            // watch()` einen neuen Stand liefert. Unabhängiger Review-Pass:
            // bei `FixedEntitlements` (Community Edition) beendet sich
            // dieser Task bereits beim Start — `FixedEntitlements::watch()`
            // droppt ihren `Sender` sofort (s. dortiger Kommentar), also
            // liefert bereits das erste `changed().await` hier ein `Err`,
            // die `while`-Schleife läuft kein einziges Mal. Kein aktiver
            // Leerlauf-Task für die gesamte App-Laufzeit, wie ein früherer
            // Kommentar hier fälschlich behauptete — die Infrastruktur
            // (Command + Event) steht trotzdem, s. Spec-Text; ein
            // künftiger `EntitlementProvider`, der seinen `Sender` am Leben
            // hält, würde diesen Task tatsächlich laufen lassen.
            let handle_for_entitlements = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut receiver = {
                    let state = handle_for_entitlements.state::<AppState>();
                    state.entitlements.watch()
                };
                while receiver.changed().await.is_ok() {
                    let current = receiver.borrow_and_update().clone();
                    tauri::Emitter::emit(&handle_for_entitlements, "entitlements:changed", current)
                        .ok();
                }
            });

            Ok(())
        })
        .manage(app_state)
        // Spec 0084, A1: die Zuordnung Sitzung → erhöhter SFTP-Kanal ist
        // eigener, von Tauri verwalteter Zustand von `app-shell` — bewusst
        // kein Feld von `AppState`, damit der Kanal auch dann hier bleibt,
        // wenn die übrige Anwendungslogik in einen Tauri-freien Crate zieht.
        .manage(crate::elevated_sftp::ElevatedSftpRegistry::default())
        .manage(edition)
        .invoke_handler(tauri::generate_handler![
            commands::list_servers,
            commands::list_ai_providers,
            commands::add_ai_provider,
            commands::update_ai_provider,
            commands::delete_ai_provider,
            commands::set_active_ai_provider,
            commands::discover_models,
            commands::test_ai_provider_credentials,
            commands::fetch_attestation_info,
            commands::connect,
            commands::confirm_host_key,
            commands::open_terminal,
            commands::terminal_input,
            commands::terminal_resize,
            commands::send_chat_message,
            commands::continue_truncated_response,
            commands::take_chat_content_into_note,
            commands::respond_to_action,
            commands::cancel_running_command,
            commands::stop_auto_continuation,
            commands::disconnect,
            commands::list_chat_sessions,
            commands::resume_chat_session,
            commands::rename_chat_session,
            commands::delete_chat_session,
            chat_retention::get_chat_session_retention_days,
            chat_retention::set_chat_session_retention_days,
            commands::list_sessions,
            commands::get_chat_history,
            commands::list_groups,
            commands::create_group,
            commands::update_group,
            commands::delete_group,
            commands::get_server,
            commands::create_server,
            commands::update_server,
            commands::delete_server,
            commands::clear_server_sudo_password,
            // Spec 0076 (BL-0221/BL-0222): Schlüsseldatei-Befund und
            // Überführung in den Schlüsselbund.
            commands::inspect_key_file,
            commands::convert_identity_file_to_keychain,
            commands::test_connection,
            commands::trust_host_key,
            commands::update_group_notes,
            commands::update_server_notes,
            commands::large_note_dialog_threshold_chars,
            commands::request_note_shrink,
            commands::update_local_server_notes,
            commands::update_local_server_tags,
            commands::list_note_revisions,
            commands::rollback_note,
            commands::preview_effective_notes,
            commands::list_rules,
            commands::create_rule,
            commands::update_rule,
            commands::delete_rule,
            commands::list_hard_blacklist,
            commands::list_known_tags,
            commands::evaluate_explained,
            commands::suggest_rule_patterns,
            commands::accept_and_create_rule,
            commands::export_document,
            commands::read_credential_file,
            // Spec 0075, §7.3: Vorschau und Ausführung des
            // `ssh_config`-Imports. Die Vorschau öffnet den Dateidialog
            // selbst; das Ausführen nimmt nur Indizes (§5.1).
            ssh_config_apply::preview_ssh_config_import,
            ssh_config_apply::apply_ssh_config_import,
            // Spec 0075, §7.4: Export nach `ssh_config`. Öffnet den
            // Speichern-Dialog selbst (§5.1-Muster).
            ssh_config_export::export_ssh_config,
            commands::get_platform,
            commands::get_app_info,
            commands::get_entitlements,
            commands::create_overlay_titlebar,
            commands::list_prompt_history,
            commands::open_log_directory,
            commands::get_keychain_status,
            commands::generate_diagnostics_bundle,
            commands::save_diagnostics_bundle,
            commands::sftp_list,
            commands::sftp_download,
            commands::sftp_download_default,
            commands::sftp_download_dir,
            commands::sftp_upload,
            commands::sftp_delete,
            commands::sftp_delete_preview,
            commands::sftp_rename,
            commands::sftp_mkdir,
            commands::sftp_read_text,
            commands::sftp_exists,
            commands::sftp_stat,
            commands::sftp_chmod,
            commands::sftp_elevation_enable,
            commands::sftp_elevation_disable,
            commands::sftp_elevation_status,
            commands::read_local_text_preview,
            commands::sftp_open_for_editing,
            commands::local_file_mtime,
            commands::close_edit_session,
            mcp_settings::get_mcp_server_settings,
            mcp_settings::set_mcp_server_enabled,
            mcp_settings::regenerate_mcp_server_token,
            mcp_settings::set_mcp_server_allowed_servers,
            mcp_settings::set_mcp_server_confirm_timeout_secs,
        ])
        .run(context)
        .expect("Fehler beim Starten der Tauri-App");
}
