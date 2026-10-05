//! Spec 0101, Teil 0 Frage 3: die Startdialoge **im Fenster**, für den
//! Passwort-Modus.
//!
//! **Warum nicht die nativen Dialoge aus `startup_prompt`:** Im
//! Passwort-Modus entsteht das Fenster, bevor der Zustand da ist (gemessen,
//! M5) — der ganze Startablauf läuft deshalb aus einem Kommando heraus, also
//! auf einem Arbeitsthread der Async-Laufzeit und nicht auf dem
//! Haupt-Thread. `rfd` trägt das auf macOS nicht. Dazu hat `rfd` keine
//! Texteingabe (§1), die Entsperr- und Einrichtemasken müssen also ohnehin
//! ins Fenster. Zwei Darstellungsarten im selben Ablauf wären die schlechtere
//! Antwort als eine.
//!
//! **Die Entscheidungslogik bleibt, wo sie ist.** Dieses Modul ist eine
//! zweite Implementierung von [`StartupPrompt`] — dasselbe Trait, das die
//! nativen Dialoge tragen. `app_logic::database_startup` und
//! `app_logic::secret_migration` sehen keinen Unterschied und wurden dafür
//! nicht geändert; §5 stellt genau diese Wahl zur Verfügung.
//!
//! **Wie das Warten funktioniert:** `ask` ist synchron (das Trait ist es),
//! läuft aber in einer asynchronen Aufgabe. Es schickt die Frage als
//! Ereignis ans Frontend und wartet auf dem Kanal — mit
//! `tokio::task::block_in_place`, derselben gemessenen Zusicherung wie beim
//! synchronen Secret-Speicher (Teil 0 Frage 2: Tauris Laufzeit ist eine
//! Multi-Thread-Laufzeit, s. `runtime_assumptions`).

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::Emitter;
use tokio::sync::oneshot;

use app_logic::database_startup::{NewMasterPassword, StartupChoice, StartupDialog, StartupPrompt};
use app_logic::startup_choice_dialogs as texts;
use app_logic::startup_error_messages::Language;
use credentials_keyring::{KeychainAvailability, KeychainUnavailableReason};
use persistence_sqlite::{detect_database_file_state, DatabaseFileState};

/// Das Ereignis, mit dem das Fenster nach einer Entscheidung gefragt wird.
pub const STARTUP_PROMPT_EVENT: &str = "startup:prompt";

/// Welche Knöpfe die Maske anbieten darf. Das Frontend bildet sie ab, es
/// erfindet keine — jeder Knopf, der hier nicht steht, darf dort nicht
/// erscheinen.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PromptKind {
    /// „Erneut versuchen“ / „Beenden“ (D1, A11).
    RetryOrQuit,
    /// „Erneut versuchen“ / „Ohne Übernahme fortfahren“ / „Beenden“ (A11.1).
    RetrySkipOrQuit,
    /// „Neu anfangen“ / „Beenden“ (D2, D3).
    StartOverOrQuit,
    /// „Neuen Schlüssel erzeugen“ / „Beenden“ (D4).
    NewKeyOrQuit,
    /// Zweite Bestätigung zu „Neu anfangen“ (A5).
    ConfirmStartOver,
    /// Zweite Bestätigung zu „Neuen Schlüssel erzeugen“ (A5/D4).
    ConfirmNewKey,
    /// Ein neues Master-Passwort eingeben (A13).
    NewMasterPassword,
    /// Nur eine Meldung mit „OK“ (A5, nach dem Umbenennen).
    Notice,
}

/// Die Frage, wie sie im Fenster erscheint.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupPromptRequest {
    pub kind: PromptKind,
    pub title: String,
    pub message: String,
}

/// Die Antwort aus dem Fenster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupPromptAnswer {
    Retry,
    Quit,
    StartOver,
    GenerateNewKey,
    ContinueWithoutMigration,
    /// Zweite Bestätigung bejaht.
    Confirm,
    /// Zweite Bestätigung abgelehnt, Maske abgebrochen, Meldung quittiert.
    Cancel,
}

/// Der Fragesteller. Hält genau **eine** offene Frage — mehr kann es nicht
/// geben, weil der Startablauf linear ist und auf jede Antwort wartet.
pub struct WindowStartupPrompt {
    app: tauri::AppHandle,
    pub db_path: PathBuf,
    pub language: Language,
    pub keychain: KeychainAvailability,
    pending: Mutex<Option<oneshot::Sender<StartupPromptAnswer>>>,
    /// Das zuletzt eingegebene neue Master-Passwort (A13). Steht getrennt
    /// von `pending`, weil es nicht in das Antwort-Ereignis gehört: Ein
    /// Passwort in einem `Serialize`-Typ wäre genau der Weg in ein DTO, den
    /// §6 ausschließt. Es kommt über ein eigenes Kommando.
    new_password: Mutex<Option<NewMasterPassword>>,
}

impl WindowStartupPrompt {
    pub fn new(
        app: tauri::AppHandle,
        db_path: PathBuf,
        language: Language,
        keychain: KeychainAvailability,
    ) -> Self {
        Self {
            app,
            db_path,
            language,
            keychain,
            pending: Mutex::new(None),
            new_password: Mutex::new(None),
        }
    }

    /// Nimmt die Antwort aus dem Fenster an. `false`, wenn gerade keine
    /// Frage offen ist — dann ist der Aufruf verspätet oder erfunden und
    /// wird verworfen, statt eine spätere Frage vorab zu beantworten.
    pub fn answer(&self, answer: StartupPromptAnswer) -> bool {
        let sender = self.pending.lock().expect("Prompt-Sperre").take();
        match sender {
            Some(sender) => sender.send(answer).is_ok(),
            None => {
                tracing::warn!("a startup-prompt answer arrived with no question open");
                false
            }
        }
    }

    /// A13: das neue Master-Passwort hinterlegen, bevor die Maske ihre
    /// Antwort schickt.
    pub fn provide_new_password(&self, password: NewMasterPassword) {
        *self.new_password.lock().expect("Passwort-Sperre") = Some(password);
    }

    fn request(&self, request: StartupPromptRequest) -> StartupPromptAnswer {
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().expect("Prompt-Sperre");
            // Eine noch offene Frage kann es nicht geben (linearer Ablauf).
            // Käme es doch dazu, ist „beenden“ die sichere Antwort auf die
            // alte — nicht ihr stilles Verschwinden.
            if let Some(previous) = pending.take() {
                let _ = previous.send(StartupPromptAnswer::Quit);
                tracing::warn!("a second startup prompt replaced one that was still open");
            }
            *pending = Some(tx);
        }

        if let Err(err) = self.app.emit(STARTUP_PROMPT_EVENT, &request) {
            // Ohne Fenster gibt es keine Antwort. „Beenden“ ist der einzige
            // Ausgang, der nichts anfasst.
            tracing::error!(error = %err, "could not show the startup prompt in the window");
            return StartupPromptAnswer::Quit;
        }

        // s. Modulkommentar: synchrones Warten in einer asynchronen
        // Aufgabe, auf der gemessenen Multi-Thread-Laufzeit.
        match tokio::task::block_in_place(|| rx.blocking_recv()) {
            Ok(answer) => answer,
            // Der Kanal ist zugegangen — das Fenster ist weg. Wieder:
            // beenden, nichts anfassen.
            Err(_) => {
                tracing::warn!("the window closed while a startup prompt was open");
                StartupPromptAnswer::Quit
            }
        }
    }

    /// D1 nennt bei einer vorhandenen Klartext-Datenbank den Verlust des
    /// bisherigen Verlaufs (A3, D1) — dieselbe Regel wie im nativen Dialog.
    fn database_is_plaintext(&self) -> bool {
        matches!(
            detect_database_file_state(&self.db_path),
            Ok(DatabaseFileState::Plaintext)
        )
    }
}

impl StartupPrompt for WindowStartupPrompt {
    fn ask(&self, dialog: StartupDialog) -> StartupChoice {
        let path = self.db_path.as_path();
        let (kind, text) = match dialog {
            StartupDialog::D1 {
                offers_password_setup,
            } => (
                PromptKind::RetryOrQuit,
                texts::d1_keychain_unreachable_text(
                    self.keychain
                        .unavailable_reason()
                        .unwrap_or(KeychainUnavailableReason::Unknown),
                    std::env::consts::OS,
                    offers_password_setup,
                    self.database_is_plaintext(),
                    self.language,
                ),
            ),
            // A11: „Dialog D1 ohne Einrichten“ — derselbe Text, andere
            // Knöpfe (A11.1 kommt im Passwort-Modus dazu).
            StartupDialog::MigrationUnreadable {
                offers_skip_migration,
            } => (
                if offers_skip_migration {
                    PromptKind::RetrySkipOrQuit
                } else {
                    PromptKind::RetryOrQuit
                },
                texts::d1_keychain_unreachable_text(
                    self.keychain
                        .unavailable_reason()
                        .unwrap_or(KeychainUnavailableReason::Unknown),
                    std::env::consts::OS,
                    false,
                    self.database_is_plaintext(),
                    self.language,
                ),
            ),
            StartupDialog::D2 => (
                PromptKind::StartOverOrQuit,
                texts::d2_not_readable_text(path, self.language),
            ),
            StartupDialog::D3 => (
                PromptKind::StartOverOrQuit,
                texts::d3_unusable_key_text(path, self.language),
            ),
            StartupDialog::D4 => (
                PromptKind::NewKeyOrQuit,
                texts::d4_new_key_for_plaintext_text(path, self.language),
            ),
        };

        let answer = self.request(StartupPromptRequest {
            kind,
            title: text.title,
            message: text.message,
        });
        // **Die Abbildung ist je Dialogart eingeschränkt**, nicht frei: Eine
        // Oberfläche, die „Neu anfangen“ auf D1 schickt, bekommt „beenden“.
        // Dieselbe Regel wie im nativen Dialog, nur hier wichtiger, weil die
        // Antwort über IPC kommt.
        match (kind, answer) {
            (PromptKind::RetryOrQuit | PromptKind::RetrySkipOrQuit, StartupPromptAnswer::Retry) => {
                StartupChoice::Retry
            }
            (PromptKind::RetrySkipOrQuit, StartupPromptAnswer::ContinueWithoutMigration) => {
                StartupChoice::ContinueWithoutMigration
            }
            (PromptKind::StartOverOrQuit, StartupPromptAnswer::StartOver) => {
                StartupChoice::StartOver
            }
            (PromptKind::NewKeyOrQuit, StartupPromptAnswer::GenerateNewKey) => {
                StartupChoice::GenerateNewKey
            }
            (_, other) => {
                if other != StartupPromptAnswer::Quit && other != StartupPromptAnswer::Cancel {
                    tracing::warn!(
                        ?kind,
                        "the window answered a startup dialog with a choice it does not offer; \
                         quitting without touching anything (Spec 0101, A3)"
                    );
                }
                StartupChoice::Quit
            }
        }
    }

    fn confirm_start_over(&self, renamed_to: Option<&str>) -> bool {
        let text = texts::start_over_confirmation_text(renamed_to, self.language);
        self.request(StartupPromptRequest {
            kind: PromptKind::ConfirmStartOver,
            title: text.title,
            message: text.message,
        }) == StartupPromptAnswer::Confirm
    }

    fn confirm_generate_new_key(&self) -> bool {
        let text = texts::new_key_confirmation_text(self.language);
        self.request(StartupPromptRequest {
            kind: PromptKind::ConfirmNewKey,
            title: text.title,
            message: text.message,
        }) == StartupPromptAnswer::Confirm
    }

    fn notify_started_over(&self, renamed_to: &str) {
        let text = texts::started_over_notice_text(renamed_to, self.language);
        let _ = self.request(StartupPromptRequest {
            kind: PromptKind::Notice,
            title: text.title,
            message: text.message,
        });
    }

    fn ask_for_new_master_password(&self) -> Option<NewMasterPassword> {
        let text = texts::master_password_setup_text(self.language);
        let answer = self.request(StartupPromptRequest {
            kind: PromptKind::NewMasterPassword,
            title: text.title,
            message: text.message,
        });
        if answer != StartupPromptAnswer::Confirm {
            // A5/A13: Abbruch heißt, dass nichts verändert ist — auch kein
            // hinterlegtes Passwort bleibt liegen.
            *self.new_password.lock().expect("Passwort-Sperre") = None;
            return None;
        }
        self.new_password.lock().expect("Passwort-Sperre").take()
    }

    fn can_ask_for_a_password(&self) -> bool {
        true
    }
}
