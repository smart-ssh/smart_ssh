//! Spec 0101, A13/A15–A18: die Kommandos rund um das Master-Passwort —
//! Entsperren beim Start und der Moduswechsel in den Einstellungen.
//!
//! **Zwei Gruppen, und das ist keine Kosmetik:**
//!
//! Die erste Gruppe läuft **vor** dem `AppState` und steht deshalb in der
//! Positivliste des [`StartupGate`](crate::startup_gate::StartupGate)
//! (A16): `get_startup_state`, `unlock_with_master_password`,
//! `answer_startup_prompt`, `quit_application`. Keines davon nimmt
//! `State<AppState>` — es gibt ihn noch nicht.
//!
//! Die zweite Gruppe sind die Einstellungen (A15/A18). Sie laufen im
//! normalen Betrieb, also **nach** der Entsperrung, und sind damit vom Tor
//! nicht betroffen.
//!
//! **Kein Passwort verlässt das Backend** (A19): Die Kommandos nehmen es
//! als `String` an (das IPC kann nichts anderes) und verpacken es
//! unmittelbar in ein `SecretString`, das beim Freigeben überschrieben
//! wird. Kein Rückgabewert und kein DTO dieses Moduls trägt ein Passwort
//! oder einen Schlüssel.

use std::sync::Arc;

use secrecy::SecretString;
use serde::Serialize;
use tauri::{Emitter, Manager};

use app_logic::database_startup::{NewMasterPassword, RootKeyAccess, StartupAbort};
use app_logic::error::{CommandError, CommandResult};
use app_logic::master_password::{self, KeyMode};
use app_logic::state::AppState;

use crate::startup_gate::StartupGate;
use crate::window_prompt::{StartupPromptAnswer, WindowStartupPrompt};

/// Das Ereignis, mit dem das Fenster erfährt, dass der Zustand jetzt steht.
pub const UNLOCKED_EVENT: &str = "startup:unlocked";

/// Fehlercodes dieser Etappe (A20).
pub const WRONG_MASTER_PASSWORD_CODE: &str = "WRONG_MASTER_PASSWORD";
pub const MASTER_PASSWORD_FILE_FAILED_CODE: &str = "MASTER_PASSWORD_FILE_FAILED";
pub const MASTER_PASSWORD_REJECTED_CODE: &str = "MASTER_PASSWORD_REJECTED";
pub const STARTUP_FAILED_CODE: &str = "STARTUP_FAILED";

/// Was zum Nachbauen des Zustands nach der Entsperrung gebraucht wird.
///
/// **Wird immer verwaltet**, auch im Schlüsselbund-Modus: Sonst wäre
/// `get_startup_state` ein Kommando mit nicht verwaltetem `State` und
/// scheiterte dort mit einer Meldung über `.manage()` statt mit einer
/// Antwort (gemessen, M1).
pub struct PendingStartup {
    inputs: crate::StartupInputs,
    entitlements: Arc<dyn ssh_manager_core::entitlements::EntitlementProvider>,
    /// Der Fragesteller im Fenster. Entsteht beim ersten Entsperrversuch,
    /// weil er den `AppHandle` braucht.
    prompt: std::sync::Mutex<Option<Arc<WindowStartupPrompt>>>,
    /// Serialisiert jeden Weg, der den Zustand aufbaut (spec-reviewer
    /// Lauf 4, Fund 6).
    ///
    /// **Ein `tokio::sync::Mutex`, nicht der aus `std`:** Der kritische
    /// Abschnitt enthält `await`-Punkte (Datenbank öffnen, Secret-Umzug,
    /// Startdialoge). Eine `std`-Sperre über ein `await` zu halten blockiert
    /// einen Laufzeit-Thread und ist in einem `Send`-Future nicht einmal
    /// erlaubt.
    ///
    /// Die Sperre deckt **beide** Wege: Entsperren und „Neu anfangen". Das
    /// ist der Punkt — der zweite benennt Dateien um, und zwei davon
    /// gleichzeitig wäre der schlechteste Augenblick für ein Rennen.
    unlock_lock: tokio::sync::Mutex<()>,
}

impl PendingStartup {
    pub fn new(
        inputs: crate::StartupInputs,
        entitlements: Arc<dyn ssh_manager_core::entitlements::EntitlementProvider>,
    ) -> Self {
        Self {
            inputs,
            entitlements,
            prompt: std::sync::Mutex::new(None),
            unlock_lock: tokio::sync::Mutex::new(()),
        }
    }

    fn prompt_for(&self, app: &tauri::AppHandle) -> Arc<WindowStartupPrompt> {
        let mut slot = self.prompt.lock().expect("Prompt-Sperre");
        if let Some(prompt) = slot.as_ref() {
            return prompt.clone();
        }
        let prompt = Arc::new(WindowStartupPrompt::new(
            app.clone(),
            self.inputs.db_path.clone(),
            self.inputs.language,
            self.inputs.keychain,
        ));
        *slot = Some(prompt.clone());
        prompt
    }
}

/// Was die Oberfläche beim Start anzeigen soll (A16/A18).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupStateDto {
    /// `true`, sobald der Zustand steht — dann zeigt die Oberfläche die App.
    pub unlocked: bool,
    /// `true`: Entsperrmaske (A16). `false` und nicht entsperrt: die
    /// Einrichtemaske aus D1 (A13).
    pub needs_unlock: bool,
    /// A18: der aktive Modus, `"password"` oder `"keychain"`.
    pub mode: &'static str,
    /// Klarstellung 9: `true`, wenn die Verpackungsdatei mit **keinem**
    /// Passwort zu öffnen ist (A3 *ungültig*). Dann zeigt die Maske kein
    /// Passwortfeld, sondern den Ausweg „Neu anfangen" (A16) — und nur
    /// dann nimmt
    /// [`start_over_from_unlock_screen`] ihn an.
    pub wrapping_unusable: bool,
    /// Die Sprache der Startmasken, `"de"` oder `"en"`.
    ///
    /// **Warum aus dem Backend und nicht aus der Einstellungsdatei:**
    /// Klarstellung 9 verlangt, dass Einstellungsdateien vor der
    /// Entsperrung nicht lesbar sind — das `store`-Plugin ist dort noch
    /// nicht registriert (s. `crate::run`). Die Sprache ist deshalb die,
    /// die der Start einmal aus der Umgebung bestimmt hat (Spec 0071,
    /// A11a/A11b: **eine** Sprachwahl für alle Startdialoge eines
    /// Programmlaufs). Nach der Entsperrung gilt wieder die gespeicherte
    /// Wahl aus Spec 0024.
    pub language: &'static str,
}

fn mode_name(mode: KeyMode) -> &'static str {
    match mode {
        KeyMode::Password => "password",
        KeyMode::Keychain => "keychain",
    }
}

/// A16/A18: Was ist zu zeigen? Das erste Kommando, das die Oberfläche beim
/// Start aufruft.
#[tauri::command]
pub fn get_startup_state(
    gate: tauri::State<'_, Arc<StartupGate>>,
    pending: tauri::State<'_, PendingStartup>,
) -> StartupStateDto {
    startup_state(&gate, &pending)
}

/// Derselbe Inhalt wie [`get_startup_state`], aber ohne `tauri::State` —
/// damit die Kommandos unten ihn nach dem Entsperren noch bilden können,
/// ohne die Zustandsgriffe weiterzugeben.
fn startup_state(gate: &StartupGate, pending: &PendingStartup) -> StartupStateDto {
    let db_path = &pending.inputs.db_path;
    let mode = master_password::key_mode(db_path);
    let unlocked = gate.is_unlocked();
    // Die Datei wird nur im gesperrten Passwort-Modus geprüft: Danach ist
    // der Zustand schon gebaut, und ein Lesefehler wäre bloß Lärm.
    let wrapping_unusable = !unlocked
        && mode == KeyMode::Password
        && master_password::wrapping_health(db_path).allows_starting_over();
    StartupStateDto {
        unlocked,
        // Klarstellung 9: Bei unbrauchbarer Datei gibt es nichts zu
        // entsperren — die Maske soll nicht nach einem Passwort fragen, das
        // nie passen kann.
        needs_unlock: !unlocked && mode == KeyMode::Password && !wrapping_unusable,
        mode: mode_name(mode),
        wrapping_unusable,
        language: match pending.inputs.language {
            app_logic::startup_error_messages::Language::De => "de",
            app_logic::startup_error_messages::Language::En => "en",
        },
    }
}

/// A16: Entsperren und den Zustand nachbauen.
///
/// **Was hier nie passiert** (A16, wörtlich): Bei einem falschen Passwort
/// wird die Datenbank nicht geöffnet, nichts umbenannt und **nie** ein
/// neuer Schlüssel erzeugt. Der Fehler geht an die Maske zurück, das Tor
/// bleibt zu, und der Nutzer kann es erneut versuchen.
#[tauri::command]
pub async fn unlock_with_master_password(
    app: tauri::AppHandle,
    password: String,
    gate: tauri::State<'_, Arc<StartupGate>>,
    pending: tauri::State<'_, PendingStartup>,
) -> CommandResult<StartupStateDto> {
    // **Ein Entsperrvorgang zur Zeit** (spec-reviewer Lauf 4, Fund 6).
    // Die `is_unlocked()`-Prüfung allein genügt nicht: Zwei gleichzeitige
    // Aufrufe kämen beide daran vorbei und bauten beide einen vollständigen
    // Zustand auf — zwei Verbindungspools, zwei Secret-Umzüge, zwei
    // Aufräumläufe auf dem Schlüsselbund. Das Schloss wird **vor** der
    // Prüfung genommen, damit der zweite Aufruf den fertigen Zustand sieht.
    let _serialized = pending.unlock_lock.lock().await;
    if gate.is_unlocked() {
        // Schon entsperrt — ein zweiter Aufruf darf den Zustand nicht
        // ersetzen (gemessen M3: `manage` würde ihn ohnehin nicht
        // ersetzen, aber der ganze Aufbau liefe erneut).
        return Ok(startup_state(&gate, &pending));
    }

    let password = SecretString::from(password);
    let db_path = pending.inputs.db_path.clone();
    let mode = master_password::key_mode(&db_path);

    // Im Passwort-Modus zuerst die Verpackung öffnen; ohne sie gibt es kein
    // K und damit keinen Weg weiter. Im Modus „Einrichten aus D1" (Teil 0
    // Frage 3) gibt es noch keine Verpackung — dort fährt der Ablauf mit
    // `Keychain` weiter und läuft erneut in D1, diesmal mit der Maske im
    // Fenster.
    let access_owner;
    let keyring = credentials_keyring::KeyringCredentialStore::new();
    let access = match mode {
        KeyMode::Password => {
            let key = master_password::unlock(&db_path, &password).map_err(to_command_error)?;
            // A17: Ein liegengebliebener Schlüsselbund-Eintrag wird
            // aufgeräumt — aber nur, wenn er gleich K ist.
            master_password::tidy_up_keychain_after_unlock(&keyring, &key);
            access_owner = key;
            RootKeyAccess::Unlocked(*access_owner.expose())
        }
        KeyMode::Keychain => RootKeyAccess::Keychain(&keyring),
    };

    // A11.1 gilt nur im Passwort-Modus.
    let offers_skip = mode == KeyMode::Password;
    assemble_and_open_the_gate(&app, &gate, &pending, access, offers_skip).await
}

/// Spec 0101, Klarstellung 9 + A16 („Neu anfangen"): der Ausweg aus einer
/// Verpackungsdatei, die **kein** Passwort mehr öffnet.
///
/// **Warum ein eigenes Kommando und nicht ein Zweig von
/// [`unlock_with_master_password`]:** Entsperren und Neuanfangen sind
/// gegensätzliche Vorgänge — das eine holt K, das andere gibt ihn auf. Sie
/// in einen Aufruf zu legen hieße, dass ein Tippfehler im Passwortfeld in
/// der Nähe eines Codepfads landet, der Daten aufgibt. Getrennt ist
/// „Daten aufgeben" eine eigene, ausdrückliche Nutzerwahl, wie A5 sie
/// verlangt — mit der zweiten Bestätigung aus dem Startablauf dahinter.
///
/// **Der Riegel:** Der Aufruf wird abgelehnt, solange die Datei brauchbar
/// ist. Die Oberfläche entscheidet das nicht; sie kann es nur anfragen.
/// Sonst wäre dieses Kommando bei bloß vergessenem Passwort ein Knopf, der
/// den Verlauf wegwirft, obwohl K noch zu holen wäre.
#[tauri::command]
pub async fn start_over_from_unlock_screen(
    app: tauri::AppHandle,
    gate: tauri::State<'_, Arc<StartupGate>>,
    pending: tauri::State<'_, PendingStartup>,
) -> CommandResult<StartupStateDto> {
    let _serialized = pending.unlock_lock.lock().await;
    if gate.is_unlocked() {
        return Ok(startup_state(&gate, &pending));
    }

    let db_path = pending.inputs.db_path.clone();
    let health = master_password::wrapping_health(&db_path);
    if !health.allows_starting_over() {
        tracing::warn!(
            ?health,
            "refusing to start over: the wrapping file next to the database may still open with \
             the right password (Spec 0101, A5/A16)"
        );
        return Err(CommandError::with_code(
            "Deine Schlüsseldatei ist in Ordnung — nur das Passwort passt nicht. Versuche es \
             erneut; es wird nichts verändert.",
            WRONG_MASTER_PASSWORD_CODE,
        ));
    }

    // `UnusableWrapping` führt die Tabelle A3 nach *ungültig* und damit nach
    // D3 bzw. D4. Der Dialog, die zweite Bestätigung und die Abfrage des
    // neuen Master-Passworts laufen über den Fragesteller im Fenster und
    // werden mit `answer_startup_prompt` beantwortet — beides steht in der
    // Positivliste des Tors.
    tracing::info!(
        ?health,
        "starting over from the unlock screen because the wrapping file cannot yield the root \
         key (Spec 0101, A3 „ungültig“, A16)"
    );
    assemble_and_open_the_gate(
        &app,
        &gate,
        &pending,
        RootKeyAccess::UnusableWrapping,
        // A11.1 gilt im Passwort-Modus.
        true,
    )
    .await
}

/// Der gemeinsame Rest von [`unlock_with_master_password`] und
/// [`start_over_from_unlock_screen`]: Zustand bauen, verwalten, Tor öffnen.
///
/// Als eigene Funktion, damit die **Reihenfolge** (Zustand, dann Tor) an
/// genau einer Stelle steht. Zwei Kopien davon wären zwei Gelegenheiten,
/// sie zu vertauschen.
async fn assemble_and_open_the_gate(
    app: &tauri::AppHandle,
    gate: &StartupGate,
    pending: &PendingStartup,
    access: RootKeyAccess<'_>,
    offers_skip: bool,
) -> CommandResult<StartupStateDto> {
    let prompt = pending.prompt_for(app);
    let state = crate::open_and_assemble(
        &pending.entitlements,
        &pending.inputs,
        access,
        prompt.as_ref(),
        offers_skip,
    )
    .await
    .map_err(|abort| startup_abort_to_command_error(&pending.inputs, abort))?;

    // **Die Reihenfolge ist wichtig:** erst der Zustand, dann das Tor.
    // Andersherum gäbe es ein Fenster, in dem ein Kommando durchkäme, für
    // das `State<AppState>` noch nicht verwaltet ist.
    if !app.manage(state) {
        // Gemessen (M3): Ein zweites `manage` ersetzt nichts. Mit dem
        // Schloss um beide Kommandos kann es dazu nur kommen, wenn der
        // Zustand aus einer früheren Entsperrung desselben Programmlaufs
        // schon steht — dann gilt die erste, und diese hier hat nichts
        // getan.
        tracing::warn!("the application state was already managed; this unlock changed nothing");
    }
    gate.unlock();
    // Klarstellung 9: Erst jetzt dürfen die Plugins da sein, die Dateien,
    // Einstellungen oder das Betriebssystem berühren (A16).
    crate::register_unlocked_plugins(app);
    tracing::info!("unlocked and assembled the application state (Spec 0101, A16)");
    let _ = app.emit(UNLOCKED_EVENT, ());

    Ok(startup_state(gate, pending))
}

/// Die Antwort auf einen Startdialog im Fenster (Teil 0 Frage 3).
///
/// Das neue Master-Passwort kommt getrennt mit — es gehört nicht in das
/// Ereignis, mit dem gefragt wurde (§6: kein Passwort in einem DTO).
#[tauri::command]
pub fn answer_startup_prompt(
    app: tauri::AppHandle,
    answer: StartupPromptAnswer,
    password: Option<String>,
    repeated: Option<String>,
    pending: tauri::State<'_, PendingStartup>,
) -> CommandResult<()> {
    let prompt = pending.prompt_for(&app);
    if let (Some(password), Some(repeated)) = (password, repeated) {
        prompt.provide_new_password(NewMasterPassword {
            password: SecretString::from(password),
            repeated: SecretString::from(repeated),
        });
    }
    if !prompt.answer(answer) {
        return Err(CommandError::with_code(
            "Es ist gerade keine Frage offen.",
            STARTUP_FAILED_CODE,
        ));
    }
    Ok(())
}

/// A16: „Beenden“ aus der Entsperrmaske.
///
/// Rückgabewert 0: Ein bewusstes Beenden ist kein Fehler.
#[tauri::command]
pub fn quit_application(app: tauri::AppHandle) {
    tracing::info!("the user quit from the unlock screen (Spec 0101, A16)");
    app.exit(0);
}

/// A18: Welcher Modus ist aktiv? Für die Einstellungen.
#[tauri::command]
pub fn get_master_password_mode(state: tauri::State<'_, AppState>) -> &'static str {
    let _ = &state;
    mode_name(master_password::key_mode(
        &persistence_sqlite::default_db_path(),
    ))
}

/// A13: Master-Passwort aus den Einstellungen einrichten.
///
/// K kommt dabei aus dem Schlüsselbund — es ist dasselbe K, mit dem die
/// Datenbank gerade offen ist, und es bleibt es (E9). Die Reihenfolge
/// (verpacken, schreiben, zurücklesen, vergleichen, dann löschen) liegt in
/// `app_logic::master_password`.
#[tauri::command]
pub fn set_up_master_password(
    password: String,
    repeated: String,
    state: tauri::State<'_, AppState>,
) -> CommandResult<&'static str> {
    let _ = &state;
    let db_path = persistence_sqlite::default_db_path();
    let keyring = credentials_keyring::KeyringCredentialStore::new();

    // **K wird gelesen, nicht erzeugt** (A3/A13): Aus den Einstellungen
    // heraus ist die Datenbank offen, es gibt also einen gültigen K. Wäre
    // er nicht lesbar, dürfte hier auf keinen Fall ein neuer entstehen —
    // die Datenbank wäre damit verloren.
    let root_key = match ssh_manager_core::crypto::read_root_key(&keyring) {
        ssh_manager_core::crypto::RootKeyState::Present(key) => key,
        other => {
            tracing::warn!(
                ?other,
                "refusing to set up a master password without a readable root key \
                 (Spec 0101, A3)"
            );
            return Err(CommandError::with_code(
                "Der Schlüssel zu deiner Datenbank ist gerade nicht lesbar. Richte das \
                 Master-Passwort später erneut ein — es wird kein neuer Schlüssel erzeugt.",
                MASTER_PASSWORD_FILE_FAILED_CODE,
            ));
        }
    };

    master_password::set_up_master_password(
        &db_path,
        &root_key,
        &SecretString::from(password),
        &SecretString::from(repeated),
        Some(&keyring),
    )
    .map_err(to_command_error)?;
    Ok("password")
}

/// A15: Passwort ändern. Die Datenbank wird dabei nicht angefasst.
#[tauri::command]
pub fn change_master_password(
    current: String,
    password: String,
    repeated: String,
    state: tauri::State<'_, AppState>,
) -> CommandResult<()> {
    let _ = &state;
    master_password::change_master_password(
        &persistence_sqlite::default_db_path(),
        &SecretString::from(current),
        &SecretString::from(password),
        &SecretString::from(repeated),
    )
    .map_err(to_command_error)
}

/// A15: zurück auf den Schlüsselbund.
#[tauri::command]
pub fn switch_to_os_keychain(
    current: String,
    state: tauri::State<'_, AppState>,
) -> CommandResult<&'static str> {
    let _ = &state;
    master_password::switch_to_keychain(
        &persistence_sqlite::default_db_path(),
        &SecretString::from(current),
        &credentials_keyring::KeyringCredentialStore::new(),
    )
    .map_err(to_command_error)?;
    Ok("keychain")
}

/// A17/A20: Der Fehler für das Frontend — **ohne** Nutzlast im Text, mit
/// einem stabilen Code.
///
/// „Passwort falsch" und „Datei beschädigt" sind derselbe Code: Sie sind
/// nicht unterscheidbar (A17), und ein zweiter Code wäre ein Orakel darüber,
/// welcher der beiden Fälle vorliegt.
fn to_command_error(err: master_password::MasterPasswordError) -> CommandError {
    use master_password::MasterPasswordError as E;
    if let Some(detail) = err.detail_for_log() {
        tracing::warn!(
            detail,
            "a master-password operation failed (Spec 0101, A13–A17)"
        );
    }
    let code = match err {
        E::WrongPasswordOrDamagedFile => WRONG_MASTER_PASSWORD_CODE,
        E::PasswordRejected(_) => MASTER_PASSWORD_REJECTED_CODE,
        E::UnusableWrappingFile
        | E::FileFailed { .. }
        | E::KeychainFailed { .. }
        | E::NotInPasswordMode
        | E::AlreadyInPasswordMode => MASTER_PASSWORD_FILE_FAILED_CODE,
    };
    CommandError::with_code(err.to_string(), code)
}

/// Ein Startabbruch aus dem Entsperr-Kommando. Anders als in `run()` endet
/// er **nicht** in `process::exit`: Das Fenster steht schon, und die
/// Entsperrmaske kann den Grund zeigen (A16: „sichtbare Meldung").
fn startup_abort_to_command_error(
    inputs: &crate::StartupInputs,
    abort: StartupAbort,
) -> CommandError {
    match abort {
        StartupAbort::UserQuit => CommandError::with_code(
            "Der Start wurde abgebrochen. Es ist nichts verändert.",
            STARTUP_FAILED_CODE,
        ),
        // Kann hier nicht vorkommen: Der Fenster-Fragesteller **kann** nach
        // einem Passwort fragen (`can_ask_for_a_password`). Sichtbar
        // scheitern statt stillschweigend nichts zu tun.
        StartupAbort::NeedsWindow => CommandError::with_code(
            "Der Start braucht eine Eingabe, die hier nicht möglich ist.",
            STARTUP_FAILED_CODE,
        ),
        StartupAbort::Fatal { kind, detail } => {
            let log_dir = app_logic::logging::default_log_dir();
            let text = app_logic::startup_error_messages::db_connect_failure_text(
                &kind,
                &inputs.db_path,
                &log_dir,
                inputs.language,
            );
            // Wie in `run()`: `detail` nur ins Log.
            tracing::error!(detail, ?kind, "the startup failed after unlocking");
            CommandError::with_code(text.message, STARTUP_FAILED_CODE)
        }
    }
}
