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
        self.ask_choice_with_extra(text, confirm, StartupChoice::Quit)
    }

    /// Mit drittem Knopf (A3, D1: „Master-Passwort einrichten“). `extra` im
    /// Text entscheidet, ob er überhaupt erscheint — ist er `None`, kann
    /// `DialogAnswer::Extra` nicht zurückkommen.
    fn ask_choice_with_extra(
        &self,
        text: texts::ChoiceDialogText,
        confirm: StartupChoice,
        extra: StartupChoice,
    ) -> StartupChoice {
        match startup_dialog::ask(
            &text.title,
            &text.message,
            &text.confirm,
            &text.cancel,
            text.extra.as_deref(),
        ) {
            DialogAnswer::Confirm => confirm,
            DialogAnswer::Extra => extra,
            DialogAnswer::Cancel => StartupChoice::Quit,
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
                // **Der dritte Knopf** (A3, D1: „Master-Passwort
                // einrichten"). Er war zurückgestellt, solange es die Maske
                // im Fenster nicht gab — ohne sie stünde der Nutzer nach dem
                // Druck vor einem Fenster ohne Eingabefeld. Mit Commit 11
                // gibt es sie: `open_or_prepare_database` antwortet auf diese
                // Wahl mit `StartupAbort::NeedsWindow`, die App startet ohne
                // Zustand, und die Startmaske setzt den Ablauf im Fenster
                // fort (`StartupScreen::SetUpMasterPassword`).
                tracing::info!(
                    offers_password_setup,
                    "D1: offering the master-password setup button; the form itself is in the \
                     window (Spec 0101, A13, §2 Frage 3)"
                );
                let text = texts::d1_keychain_unreachable_text(
                    self.keychain
                        .unavailable_reason()
                        .unwrap_or(KeychainUnavailableReason::Unknown),
                    std::env::consts::OS,
                    offers_password_setup,
                    self.database_is_plaintext(),
                    self.language,
                );
                // `extra` im Text entscheidet, ob der Knopf erscheint: Ist
                // `offers_password_setup` falsch, ist er `None`, und
                // `DialogAnswer::Extra` kann nicht zurückkommen (s.
                // `ask_choice_with_extra`). Die Einschränkung aus A3/D1
                // („**nur** bei Datei *fehlt* oder *Klartext* und Grund
                // `NoSecretServiceProvider`/`NoSessionBus`") liegt damit
                // unverändert in `database_startup`, nicht hier.
                self.ask_choice_with_extra(
                    text,
                    StartupChoice::Retry,
                    StartupChoice::SetUpMasterPassword,
                )
            }
            // A11: „Dialog D1 ohne Einrichten“. Der dritte Knopf aus A11.1
            // gehört zum Passwort-Modus und erscheint dort im Fenster —
            // nativ gibt es ihn nicht.
            StartupDialog::MigrationUnreadable { .. } => {
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

    fn confirm_start_over(&self, renamed_to: Option<&str>) -> bool {
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

    /// Issue #113: einmaliger Hinweis nach der Umstellung, wie A5.
    fn notify_unreadable_history_removed(&self, removed: u64) {
        let text = texts::unreadable_history_removed_notice_text(removed, self.language);
        startup_dialog::show_info(&text.title, &text.message);
    }

    /// §1: rfd hat keine Texteingabe. Die Maske erscheint im Fenster
    /// (`crate::window_prompt`); dieser Fragesteller kommt nie dorthin, weil
    /// [`Self::can_ask_for_a_password`] `false` liefert.
    fn ask_for_new_master_password(
        &self,
    ) -> Option<app_logic::database_startup::NewMasterPassword> {
        tracing::error!(
            "the native startup dialog cannot ask for a password; this should have been \
             deferred to the window (Spec 0101, §2 Frage 3)"
        );
        None
    }

    fn can_ask_for_a_password(&self) -> bool {
        false
    }
}
