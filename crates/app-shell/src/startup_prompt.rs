//! Spec 0101, A3/A5: die native Hälfte der Startdialoge — der einzige Teil
//! des Startablaufs, der ein Fenster braucht.
//!
//! Text und Entscheidung liegen woanders
//! (`app_logic::startup_choice_dialogs` bzw.
//! `app_logic::database_startup`); hier steht nur die Zuordnung
//! „welcher Fall ruft welchen Text und welche Knöpfe auf". Dadurch prüfen
//! T3/T7/T8 den ganzen Ablauf ohne Fenster, und diese Datei enthält keine
//! Logik, die ein Test abdecken müsste.

use std::path::{Path, PathBuf};

use app_logic::database_startup::{StartupChoice, StartupDialog, StartupPrompt};
use app_logic::startup_choice_dialogs as texts;
use app_logic::startup_error_messages::Language;
use credentials_keyring::{KeychainAvailability, KeychainUnavailableReason};
use persistence_sqlite::{detect_database_file_state, DatabaseFileState};

use crate::startup_dialog::{self, DialogAnswer};

pub struct NativeStartupPrompt {
    pub db_path: PathBuf,
    pub language: Language,
    /// Für die Ursache in D1 (Spec 0071, A12).
    pub keychain: KeychainAvailability,
}

impl NativeStartupPrompt {
    /// D1 nennt bei einer vorhandenen Klartext-Datenbank den Verlust des
    /// bisherigen Verlaufs (A3, D1 letzter Satz). Der Dateizustand wird
    /// dafür erneut gelesen statt durchgereicht — er ändert die Datei nicht
    /// (A3), und ein zweiter Weg, ihn weiterzugeben, wäre eine zweite
    /// Stelle, an der er veralten kann.
    fn database_is_plaintext(&self) -> bool {
        matches!(
            detect_database_file_state(&self.db_path),
            Ok(DatabaseFileState::Plaintext)
        )
    }

    fn ask_choice(&self, text: texts::ChoiceDialogText, confirm: StartupChoice) -> StartupChoice {
        match startup_dialog::ask(
            &text.title,
            &text.message,
            &text.confirm,
            &text.cancel,
            text.extra.as_deref(),
        ) {
            DialogAnswer::Confirm => confirm,
            // Der dritte Knopf gibt es in Etappe 1/2 nicht (`extra` ist
            // `None`), also kann `Extra` hier nicht vorkommen. Käme er doch,
            // wäre „beenden" die sichere Antwort — nicht die Handlung.
            DialogAnswer::Extra | DialogAnswer::Cancel => StartupChoice::Quit,
        }
    }
}

impl StartupPrompt for NativeStartupPrompt {
    fn ask(&self, dialog: StartupDialog) -> StartupChoice {
        let path: &Path = &self.db_path;
        match dialog {
            StartupDialog::D1 {
                offers_password_setup,
            } => {
                // Etappe 3 bringt das Einrichten; bis dahin wird der dritte
                // Knopf **nicht** gezeigt. Der Wert wird trotzdem geloggt,
                // damit nachvollziehbar ist, dass die Entscheidung ihn schon
                // berechnet.
                tracing::info!(
                    offers_password_setup,
                    "D1: master-password setup arrives with stage 3 (Spec 0101, A13)"
                );
                let text = texts::d1_keychain_unreachable_text(
                    self.keychain
                        .unavailable_reason()
                        .unwrap_or(KeychainUnavailableReason::Unknown),
                    std::env::consts::OS,
                    false,
                    self.database_is_plaintext(),
                    self.language,
                );
                self.ask_choice(text, StartupChoice::Retry)
            }
            StartupDialog::D2 => self.ask_choice(
                texts::d2_not_readable_text(path, self.language),
                StartupChoice::StartOver,
            ),
            StartupDialog::D3 => self.ask_choice(
                texts::d3_unusable_key_text(path, self.language),
                StartupChoice::StartOver,
            ),
            StartupDialog::D4 => self.ask_choice(
                texts::d4_new_key_for_plaintext_text(path, self.language),
                StartupChoice::GenerateNewKey,
            ),
        }
    }

    fn confirm_start_over(&self, renamed_to: &str) -> bool {
        let text = texts::start_over_confirmation_text(renamed_to, self.language);
        startup_dialog::ask(
            &text.title,
            &text.message,
            &text.confirm,
            &text.cancel,
            None,
        ) == DialogAnswer::Confirm
    }

    fn confirm_generate_new_key(&self) -> bool {
        let text = texts::new_key_confirmation_text(self.language);
        startup_dialog::ask(
            &text.title,
            &text.message,
            &text.confirm,
            &text.cancel,
            None,
        ) == DialogAnswer::Confirm
    }

    fn notify_started_over(&self, renamed_to: &str) {
        let text = texts::started_over_notice_text(renamed_to, self.language);
        startup_dialog::show_info(&text.title, &text.message);
    }
}
