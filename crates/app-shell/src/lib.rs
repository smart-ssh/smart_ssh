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
/// Spec 0101, Teil 0 Frage 2: die gemessene Annahme über Tauris
/// Async-Runtime, auf der der synchrone Secret-Speicher steht.
#[cfg(test)]
mod runtime_assumptions;
/// Spec 0075, §7.3: den bestätigten Importplan ausführen — der einzige
/// Schritt, in dem überhaupt eine Schlüsseldatei geöffnet wird (§5.1).
mod ssh_config_apply;
/// Spec 0075, §7.4: Export nach `ssh_config` — Dateidialog,
/// `~/.ssh/config`-Ablehnung, Schreiben. Die Abbildung selbst liegt in
/// `ssh_manager_core::profiles::ssh_config::export`.
mod ssh_config_export;
mod startup_dialog;
/// Spec 0101, A16: das Tor, das vor der Entsperrung jedes Kommando außer
/// Entsperren, Beenden und Neu-anfangen abweist.
mod startup_gate;
/// Spec 0101, A3/A5: die nativen Startdialoge zu den Fällen D1–D4 —
/// Zuordnung von Fall zu Text und Knöpfen, ohne eigene Logik.
mod startup_prompt;
#[cfg(test)]
mod test_support;
/// Spec 0101, Teil 0 Frage 3: die Startdialoge im Fenster, fuer den
/// Passwort-Modus.
mod window_prompt;
mod wiring;

pub use wiring::{Edition, Wiring};

use std::sync::Arc;

use credentials_keyring::KeyringCredentialStore;
use persistence_sqlite::default_db_path;

use app_logic::confirmation::ConfirmationRegistry;
use app_logic::host_key_store::FileHostKeyStore;
use app_logic::session::SessionManager;
use app_logic::state::AppState;

/// Die Angaben, die der Startablauf einmal pro Programmlauf aus der
/// Umgebung liest (Spec 0071 A11a/A3) — vor jeder Entscheidung darüber, wie
/// K beschafft wird.
///
/// **Eigener Typ seit Spec 0101 Etappe 3:** Im Passwort-Modus wird der
/// Startablauf in zwei Hälften geteilt (Teil 0 Frage 3). Die erste läuft in
/// [`run`] vor dem Fenster, die zweite im Entsperr-Kommando. Beide brauchen
/// dieselben Angaben, und sie dürfen sich nicht unterscheiden — die
/// Umgebung wird deshalb genau einmal gelesen.
pub(crate) struct StartupInputs {
    pub(crate) db_path: std::path::PathBuf,
    pub(crate) language: app_logic::startup_error_messages::Language,
    pub(crate) keychain: credentials_keyring::KeychainAvailability,
}

fn startup_inputs() -> StartupInputs {
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

    StartupInputs {
        db_path,
        language,
        keychain,
    }
}

/// Spec 0101, §5 Schritte 4–9: ab dem Öffnen der Datenbank bis zum fertigen
/// `AppState`.
///
/// `async`, nicht synchron mit `block_on` (wie bis Etappe 2): Im
/// Passwort-Modus läuft diese Hälfte **innerhalb** von Tauris Laufzeit, aus
/// dem Entsperr-Kommando heraus — ein `tauri::async_runtime::block_on`
/// würde dort panicken („cannot start a runtime from within a runtime“).
/// [`run`] ruft sie außerhalb der Laufzeit über ein `block_on` auf.
///
/// `offers_skip` ist A11.1 und gilt nur im Passwort-Modus.
pub(crate) async fn open_and_assemble(
    entitlements: &Arc<dyn ssh_manager_core::entitlements::EntitlementProvider>,
    inputs: &StartupInputs,
    access: app_logic::database_startup::RootKeyAccess<'_>,
    prompt: &dyn app_logic::database_startup::StartupPrompt,
    offers_skip: bool,
) -> Result<AppState, app_logic::database_startup::StartupAbort> {
    let db_path = inputs.db_path.clone();
    let keychain = inputs.keychain;

    // **Nur noch für K** (Spec 0101, E2/A9): Der Schlüsselbund des
    // Betriebssystems trägt ab Etappe 2 ausschließlich den Wurzelschlüssel.
    // Die Secrets selbst liegen in der verschlüsselten Datenbank, hinter
    // `AppState.credential_store` (s. unten).
    let keyring_store = KeyringCredentialStore::new();

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
    // **Die Fehlerbehandlung liegt jetzt beim Aufrufer**, nicht hier: Im
    // Schlüsselbund-Modus endet ein fataler Startfehler in einem nativen
    // Dialog und `process::exit` (`run`), im Passwort-Modus als Meldung in
    // der Entsperrmaske (A16: „sichtbare Meldung"). Ein `process::exit` aus
    // einem Kommando heraus würde dort das Fenster wegreißen, bevor der
    // Nutzer den Grund gelesen hat.
    let opened =
        app_logic::database_startup::open_or_prepare_database(&db_path, access, keychain, prompt)
            .await?;
    tracing::info!("SQLite database connected");
    let profile_store = opened.store;
    let chat_content_key = opened.root_key;
    let ai_provider_store = profile_store.ai_provider_store();
    let policy_store = profile_store.policy_store();

    // Spec 0101, A9: Der produktive `CredentialStore` ist ab hier die
    // verschlüsselte Datenbank, nicht mehr der Schlüsselbund.
    //
    // **Der Griff auf die Runtime** (Teil 0, Frage 2): Diese Funktion ist
    // `async`, läuft also immer **innerhalb** der Laufzeit — `Handle::
    // current()` ist hier gültig (bis Etappe 2 stand hier ein eigenes
    // `block_on`, weil der Aufbau synchron außerhalb der Laufzeit lief).
    // Welchen Weg der Store mit dem Griff nimmt und warum, steht im
    // Modul-Kommentar von `persistence_sqlite::credential_store`.
    let runtime_handle = tokio::runtime::Handle::current();
    let credential_store = profile_store.credential_store(runtime_handle);

    // Spec 0101, A10/A11 (§5, Schritt 7): der einmalige Umzug der Secrets
    // aus dem Schlüsselbund in die Datenbank — **vor** dem übrigen Zustand
    // und damit vor jedem Kommando, das ein Secret lesen könnte. Die
    // Entscheidungen liegen Tauri-frei in `app_logic::secret_migration`;
    // hier wird nur derselbe native Dialog beigesteuert wie bei A3.
    //
    // Ein Lesefehler endet in D1 ohne Einrichten (A11) und damit entweder
    // in einem erneuten Versuch oder im Beenden — nie in einer laufenden
    // App, die gespeicherte Passwörter nicht mehr findet.
    app_logic::secret_migration::migrate_secrets_into_database(
        &profile_store,
        &keyring_store,
        &credential_store,
        prompt,
        offers_skip,
    )
    .await?;

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
    // Eine Stelle für die Regel „`host_keys.json` neben der Datenbank",
    // damit der Fehlertext denselben Pfad nennt, den diese Zeile lädt
    // (spec-reviewer Lauf 4, Fund 9).
    let host_key_path = app_logic::startup_error_messages::host_key_store_path(db_path);
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
                inputs.language,
            );
            tracing::error!(error = %err, "fatal: host-key store failed to load");
            // Seit Etappe 3 ein regulärer Abbruch statt `process::exit`
            // hier: Der Aufrufer entscheidet, ob daraus ein nativer Dialog
            // oder eine Meldung im Fenster wird (s. Kommentar beim Öffnen
            // der Datenbank). Der Text steht schon fest, deshalb reist er
            // als `detail` mit — `kind` bleibt `Other`, weil
            // `host_key_store_failure_text` seinen eigenen Text hat und der
            // Aufrufer ihn über `host_key_store_failure` erneut bildet.
            return Err(app_logic::database_startup::StartupAbort::Fatal {
                kind: persistence_sqlite::ConnectFailureKind::HostKeyStoreFailed,
                detail: format!("{}: {err}", text.title),
            });
        }
    };
    tracing::info!("host-key store loaded");

    let app_state = AppState {
        sessions: SessionManager::new(),
        // Klarstellung 9: die Kennung des K, mit dem diese Datenbank gerade
        // geöffnet wurde — damit das Einrichten aus den Einstellungen
        // prüfen kann, dass es denselben Schlüssel verpackt (Fund 10).
        root_key_fingerprint: ssh_manager_core::crypto::root_key_fingerprint(&chat_content_key),
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
        entitlements: entitlements.clone(),
        pending_host_key_confirmations: ConfirmationRegistry::new(),
        pending_action_confirmations: ConfirmationRegistry::new(),
        running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
        mcp: app_logic::state::McpState::default(),
        rate_limit_registry: ai_providers::RateLimitRegistry::new(),
        // Spec 0075, §5.1: leer, bis eine Vorschau gelaufen ist.
        pending_ssh_config_import: std::sync::Mutex::new(None),
    };
    Ok(app_state)
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

    // Spec 0101, Etappe 3 (Teil 0 Frage 3): Der Startablauf hat seit dem
    // Master-Passwort zwei Formen.
    //
    // **Schlüsselbund-Modus** (Standard, E8): unverändert — der `AppState`
    // entsteht hier, vor dem Fenster, mit den nativen Dialogen.
    //
    // **Passwort-Modus** (Verpackungsdatei vorhanden, §5): Hier gibt es vor
    // der Entsperrung kein K und damit keine offene Datenbank. Die App
    // startet ohne `AppState` (gemessen, M5), zeigt die Entsperrmaske, und
    // `unlock_with_master_password` baut den Zustand danach nach
    // (`AppHandle::manage`, gemessen M2). Bis dahin hält der
    // [`StartupGate`](crate::startup_gate::StartupGate) jedes Kommando auf
    // (A16).
    //
    // `_log_guard` bleibt hier und wandert **nicht** mehr in den Aufbau:
    // Der Aufbau meldet Fehler jetzt als Rückgabewert, und nur die beiden
    // `process::exit`-Stellen unten brauchen den Guard zum Flushen.
    let inputs = startup_inputs();
    let mode = app_logic::master_password::key_mode(&inputs.db_path);
    tracing::info!(?mode, "determined how the root key is kept (Spec 0101, §5)");

    let app_state = match mode {
        app_logic::master_password::KeyMode::Password => None,
        app_logic::master_password::KeyMode::Keychain => {
            let keyring = KeyringCredentialStore::new();
            let prompt = crate::startup_prompt::NativeStartupPrompt {
                db_path: inputs.db_path.clone(),
                language: inputs.language,
                keychain: inputs.keychain,
            };
            // `block_on`: `run()` läuft ohne `#[tokio::main]`, also außerhalb
            // jeder Laufzeit (s. `runtime_assumptions`).
            match tauri::async_runtime::block_on(open_and_assemble(
                &wiring.entitlements,
                &inputs,
                app_logic::database_startup::RootKeyAccess::Keychain(&keyring),
                &prompt,
                // A11.1 gilt nur im Passwort-Modus.
                false,
            )) {
                Ok(state) => Some(state),
                // Der Nutzer hat „Beenden" gewählt. Es ist bereits alles
                // gesagt — ein zweiter Dialog wäre nur Lärm. Rückgabewert 0,
                // weil ein bewusstes Beenden kein Fehler ist.
                Err(app_logic::database_startup::StartupAbort::UserQuit) => {
                    tracing::info!("startup aborted by the user");
                    // `process::exit` führt keine Destruktoren aus — ohne
                    // dieses `drop` ginge der Log-Puffer verloren.
                    drop(_log_guard);
                    std::process::exit(0);
                }
                // Teil 0 Frage 3: „Master-Passwort einrichten" aus D1
                // braucht eine Texteingabe, die rfd nicht hat. Es ist
                // nichts verändert; die App startet ohne Zustand und
                // wiederholt den Ablauf aus dem Fenster.
                Err(app_logic::database_startup::StartupAbort::NeedsWindow) => {
                    tracing::info!(
                        "continuing the startup in the window so a master password can be \
                         entered (Spec 0101, A13)"
                    );
                    None
                }
                Err(app_logic::database_startup::StartupAbort::Fatal { kind, detail }) => {
                    let log_dir = app_logic::logging::default_log_dir();
                    let text = app_logic::startup_error_messages::db_connect_failure_text(
                        &kind,
                        &inputs.db_path,
                        &log_dir,
                        inputs.language,
                    );
                    // `detail` **nur** ins Log: Er kann einen
                    // Bibliothekstext enthalten; der Dialog nennt Ursache,
                    // Datenpfad und nächsten Schritt (Spec 0059).
                    tracing::error!(detail, ?kind, "fatal: database startup failed");
                    drop(_log_guard);
                    crate::startup_dialog::show_fatal_error_and_exit(&text.title, &text.message);
                }
            }
        }
    };

    // A16: offen, wenn der Zustand schon steht — sonst zu, bis entsperrt
    // ist.
    let gate = std::sync::Arc::new(crate::startup_gate::StartupGate::new(app_state.is_some()));
    let pending =
        crate::commands::master_password::PendingStartup::new(inputs, wiring.entitlements.clone());
    let plugins = wiring.plugins;

    let builder = tauri::Builder::default();
    let builder = plugins
        .into_iter()
        .fold(builder, |builder, plugin| plugin(builder));

    // Spec 0101, A16 / Klarstellung 9: **Die Plugins, die Dateien,
    // Einstellungen oder das Betriebssystem berühren, werden im
    // Passwort-Modus erst nach der Entsperrung registriert.** Im
    // Schlüsselbund-Modus bleibt es bei der Registrierung hier — dort gibt
    // es nichts zu sperren, und der Normalpfad soll unverändert bleiben.
    //
    // Warum nicht über das Tor: Es sieht Plugin-Kommandos nicht (gemessen
    // M7, s. `startup_gate`-Modulkommentar). Warum nicht über die ACL: Die
    // Capability-Datei gilt für beide Modi; die Rechte dort zu entfernen und
    // zur Laufzeit nachzureichen (`Manager::add_capability`, gemessen M8)
    // hieße, im `setup()`-Haken gegen die nebenläufig ladende Webview zu
    // rennen — und die Sprachwahl (Spec 0024) ruft `store` und `os` genau
    // beim Start. Das Risiko läge dann auf dem Normalpfad. Gemessen M9: ein
    // Plugin lässt sich nach `build()` nachregistrieren, und vorher ist sein
    // Kommando unerreichbar („plugin store not found") — selbst wenn die ACL
    // es erlaubt.
    let defer_plugins = app_state.is_none();
    let builder = if defer_plugins {
        tracing::info!(
            "the application starts locked; the plugins that touch files, settings or the \
             operating system are registered only after unlocking (Spec 0101, A16)"
        );
        builder
    } else {
        register_unlocked_plugins_on(builder)
    };

    builder
        // Entscheidung für tauri-plugin-decoration statt tauri-plugin-decorum:
        // tauri-plugin-decorum (v0.1.6) wird nicht mehr aktiv gepflegt und wirft Build-Fehler
        // bei modernen Rust-Toolchains/macOS-SDKs. tauri-plugin-decoration (v3.0.5) ist aktiv
        // gepflegt, unterstützt Tauri v2.10+ und verwaltet native macOS-Ampel-Insets sowie Windows Snap Layouts.
        // **Bleibt auch im gesperrten Zustand registriert:** Das Plugin
        // gestaltet den Fensterrahmen (Ampel-Insets, Snap-Layouts). Es liest
        // keine Datei, keine Einstellung und keinen Zustand des
        // Betriebssystems — und die Entsperrmaske braucht denselben Rahmen
        // wie die App, sonst springt das Fenster beim Entsperren.
        .plugin(tauri_plugin_decoration::init())
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

            // Spec 0101, A16: **Nichts davon im gesperrten Zustand.** Alle
            // Aufgaben unten brauchen den `AppState`, und A16 verlangt
            // ausdrücklich, dass der MCP-Server vor der Entsperrung nicht
            // läuft. Steht der Zustand noch nicht, übernimmt
            // `unlock_with_master_password` diese Schritte nach dem
            // `manage` — sie stehen deshalb in einer eigenen Funktion.
            if app.try_state::<AppState>().is_some() {
                crate::spawn_post_startup_tasks(app.handle());
            } else {
                tracing::info!(
                    "the application is locked; no background task, and no MCP server, until \
                     it is unlocked (Spec 0101, A16)"
                );
            }

            Ok(())
        })
        .manage(gate.clone())
        .manage(pending)
        // Spec 0101, A16 / Klarstellung 9: Der Merkzettel ist **nur** da,
        // wenn die Plugins aufgeschoben wurden — `register_unlocked_plugins`
        // hängt daran, ob es etwas nachzuholen gibt.
        .manage(UnlockedPluginsPendingSlot(defer_plugins))
        // Spec 0101, Klarstellung 9 (Fund 8): Ab hier gehört der
        // Flush-Wächter dem verwalteten Zustand, damit `quit_application`
        // den Puffer schreiben kann — `AppHandle::exit` führt keine
        // Destruktoren aus. Die beiden `process::exit`-Stellen oben liegen
        // vor dieser Zeile und geben ihn weiter selbst frei.
        .manage(app_logic::logging::LogFlushOnDemand::new(_log_guard))
        // Spec 0084, A1: die Zuordnung Sitzung → erhöhter SFTP-Kanal ist
        // eigener, von Tauri verwalteter Zustand von `app-shell` — bewusst
        // kein Feld von `AppState`, damit der Kanal auch dann hier bleibt,
        // wenn die übrige Anwendungslogik in einen Tauri-freien Crate zieht.
        .manage(crate::elevated_sftp::ElevatedSftpRegistry::default())
        .manage(edition)
        // Spec 0101, A16: Das Tor sitzt **vor** dem erzeugten Verteiler
        // (gemessen, M4). Die Begründung, warum es nicht genügt, sich auf
        // den fehlenden `AppState` zu verlassen, steht im Modulkommentar
        // von `startup_gate`.
        .invoke_handler(gated(gate, tauri::generate_handler![
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
            // Spec 0101, Etappe 3: Entsperren und Moduswechsel. Die ersten
            // fünf stehen in der Positivliste des Tors (A16).
            commands::master_password::get_startup_state,
            commands::master_password::unlock_with_master_password,
            commands::master_password::answer_startup_prompt,
            commands::master_password::quit_application,
            commands::master_password::start_over_from_unlock_screen,
            commands::master_password::get_master_password_mode,
            commands::master_password::set_up_master_password,
            commands::master_password::change_master_password,
            commands::master_password::switch_to_os_keychain,
        ]))
        .run(context)
        .expect("Fehler beim Starten der Tauri-App");
}

/// Spec 0101, A16: die Hintergrundaufgaben, die einen fertigen `AppState`
/// brauchen.
///
/// **Eigene Funktion und nicht mehr im `setup`-Block:** Im Passwort-Modus
/// gibt es den Zustand dort noch nicht. Jede dieser Aufgaben holte ihn mit
/// `state::<AppState>()`, was ohne `manage` panickt — und der MCP-Server
/// darf vor der Entsperrung ausdrücklich nicht laufen (A16). Nach der
/// Entsperrung ruft `unlock_with_master_password` dieselbe Funktion auf, es
/// gibt also keinen zweiten Pfad, der auseinanderlaufen könnte.
pub(crate) fn spawn_post_startup_tasks(app: &tauri::AppHandle) {
    use tauri::Manager;

    // Spec 0028, Abschnitt 9: ein beim letzten Beenden aktivierter
    // MCP-Server bleibt über einen Neustart hinweg aktiv, ohne manuelles
    // erneutes Anschalten.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::mcp_settings::autostart_if_enabled(&handle).await;
    });

    let app = app.clone();
    // Spec 0034, Abschnitt 5: Aufbewahrungs-Aufräum-Job beim
    // App-Start — No-op, solange keine Aufbewahrungsdauer
    // konfiguriert ist (Default).
    let handle_for_retention = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle_for_retention.state::<AppState>();
        crate::chat_retention::cleanup_old_chat_sessions_on_startup(&handle_for_retention, &state)
            .await;
    });

    // Spec 0040, Abschnitt 3: einmalige, idempotente Migration
    // bestehender Klartext-Zeilen in `prompt_history` — No-op,
    // sobald alle Zeilen bereits verschlüsselt sind (jeder Start
    // danach prüft erneut, findet aber nichts mehr zu tun).
    let handle_for_prompt_history_migration = app.clone();
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
    let handle_for_entitlements = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut receiver = {
            let state = handle_for_entitlements.state::<AppState>();
            state.entitlements.watch()
        };
        while receiver.changed().await.is_ok() {
            let current = receiver.borrow_and_update().clone();
            tauri::Emitter::emit(&handle_for_entitlements, "entitlements:changed", current).ok();
        }
    });
}

/// Spec 0101, A16 / Klarstellung 9: die Plugins, die **nicht** vor der
/// Entsperrung erreichbar sein dürfen — „kein Plugin-Kommando, das Dateien,
/// Einstellungen oder das Betriebssystem berührt".
///
/// Eine Funktion, kein zweimal getippter Block: Beide Wege (Bauzeit im
/// Schlüsselbund-Modus, nach dem Entsperren im Passwort-Modus) müssen
/// dieselbe Liste registrieren. Zwei Listen wären zwei Gelegenheiten, dass
/// eine davon ein Plugin behält oder verliert.
fn register_unlocked_plugins_on(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
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
        //
        // **Der Grund, warum diese Liste existiert** (Klarstellung 9):
        // Hierüber ist die Einstellungsdatei lesbar, und ein altes
        // `settings.json` aus der Zeit vor A12 kann noch den MCP-Token
        // tragen. Vor der Entsperrung ist das Plugin deshalb nicht da.
        .plugin(tauri_plugin_store::Builder::new().build())
        // Spec 0024, Abschnitt 4: liefert die System-Locale für die
        // Sprachermittlung beim ersten Start (`frontend/src/i18n.ts`).
        .plugin(tauri_plugin_os::init())
        // Spec 0028, Abschnitt 9a: native Toast-Benachrichtigung bei einer
        // wartenden MCP-Bestätigung (s. `crate::mcp_backend`). Vor der
        // Entsperrung gibt es nichts zu melden — der MCP-Server läuft dort
        // ohnehin nicht (A16).
        .plugin(tauri_plugin_notification::init())
}

/// Dasselbe wie [`register_unlocked_plugins_on`], nur nach `build()`
/// (gemessen M9: `AppHandle::plugin` wirkt dort).
///
/// Wird aus dem Entsperrpfad gerufen, **nachdem** das Tor offen ist. Ein
/// Fehler hier ist sichtbar statt still: Ohne diese Plugins fehlen der
/// Oberfläche Sprachwahl, Dateiauswahl und Benachrichtigungen, und das soll
/// im Log stehen statt als rätselhaft fehlende Funktion zu erscheinen.
pub(crate) fn register_unlocked_plugins(app: &tauri::AppHandle) {
    use tauri::Manager;

    // Im Schlüsselbund-Modus stehen sie schon seit der Bauzeit. Ein
    // zweiter Versuch würde scheitern („plugin … already registered"),
    // deshalb wird nur im aufgeschobenen Fall registriert.
    match app.try_state::<UnlockedPluginsPendingSlot>() {
        Some(slot) if slot.0 => {}
        _ => return,
    }
    let results: Vec<Result<(), tauri::Error>> = vec![
        app.plugin(tauri_plugin_dialog::init()),
        app.plugin(tauri_plugin_opener::init()),
        app.plugin(tauri_plugin_store::Builder::new().build()),
        app.plugin(tauri_plugin_os::init()),
        app.plugin(tauri_plugin_notification::init()),
    ];
    for result in results {
        if let Err(err) = result {
            tracing::error!(
                error = %err,
                "a plugin that is registered only after unlocking could not be registered \
                 (Spec 0101, A16)"
            );
        }
    }
    tracing::info!(
        "registered the plugins that touch files, settings or the operating system after \
         unlocking (Spec 0101, A16)"
    );
}

/// Merkzettel: Sind die Plugins aus [`register_unlocked_plugins_on`]
/// aufgeschoben und nach dem Entsperren nachzuholen?
///
/// Ein eigener Typ statt eines `bool` im `PendingStartup`: So steht die
/// Information dort, wo [`register_unlocked_plugins`] sie findet — ohne
/// dass die Funktion den Startzustand kennen muss. **Immer** verwaltet,
/// auch mit `false`: Ein nur manchmal vorhandener Zustand ist in Tauri eine
/// Fehlerquelle (gemessen M1).
pub(crate) struct UnlockedPluginsPendingSlot(pub bool);

/// Spec 0101, A16: legt das Tor vor den von `generate_handler!` erzeugten
/// Verteiler.
///
/// Als eigene Funktion, damit die Reihenfolge an einer Stelle steht und
/// nicht in einem Schließungsausdruck mitten in der Builder-Kette
/// verschwindet: **erst** prüfen, **dann** weiterleiten.
fn gated<F>(
    gate: std::sync::Arc<crate::startup_gate::StartupGate>,
    inner: F,
) -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static
where
    F: Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
{
    move |invoke| {
        let command = invoke.message.command();
        if !gate.allows(command) {
            tracing::warn!(
                command,
                "a command was refused because the application is locked (Spec 0101, A16)"
            );
            invoke.resolver.reject(crate::startup_gate::APP_LOCKED_CODE);
            return true;
        }
        inner(invoke)
    }
}
