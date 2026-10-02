//! Spec 0101, A3/A5: die Texte der Startdialoge, in denen der Nutzer
//! **wählt** (D1–D4 und die zweiten Bestätigungen).
//!
//! **Eigene Datei neben `startup_error_messages`:** Dort stehen Dialoge,
//! die nur melden und dann beenden (Spec 0059). Diese hier stellen eine
//! Frage, deren Antwort darüber entscheidet, ob eine Datei umbenannt oder
//! ein Schlüssel ersetzt wird — die Knopfbeschriftungen sind damit Teil der
//! Anforderung, nicht Kosmetik, und gehören in denselben testbaren,
//! Tauri-freien Bereich wie der Text.
//!
//! DE und EN wie überall in den Startdialogen (A20); die englischen
//! Fassungen sind **Übersetzungen**, keine Neufassungen.

use std::path::Path;

use credentials_keyring::KeychainUnavailableReason;

use crate::startup_error_messages::{
    cannot_start_title, keychain_unavailable_text, sanitize_path_for_display, DialogText, Language,
};

/// Ein Startdialog mit Wahl. Anders als [`DialogText`] (nur melden, dann
/// beenden) trägt er die Beschriftungen seiner Knöpfe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceDialogText {
    pub title: String,
    pub message: String,
    /// Der Knopf, der die Handlung auslöst.
    pub confirm: String,
    /// Der Knopf, der nichts tut.
    pub cancel: String,
    /// Ein dritter Knopf, falls der Fall einen hat (D1 ab Etappe 3).
    pub extra: Option<String>,
}

fn quit_label(language: Language) -> String {
    match language {
        Language::De => "Beenden".to_string(),
        Language::En => "Quit".to_string(),
    }
}

fn cancel_label(language: Language) -> String {
    match language {
        Language::De => "Abbrechen".to_string(),
        Language::En => "Cancel".to_string(),
    }
}

/// Spec 0101, D1: „Erneut versuchen“ / „Beenden“, mit der Ursache nach
/// Spec 0071 A12.
///
/// **Der Text sagt ausdrücklich, dass nichts angefasst wurde** (E3). Ohne
/// diesen Satz ist die naheliegende Reaktion auf einen Startfehler, von Hand
/// an den Dateien „zu retten“ — und das ist genau der Weg, auf dem Daten
/// tatsächlich verloren gehen.
///
/// `offers_password_setup` entscheidet über den dritten Knopf (A3, D1). In
/// Etappe 1/2 gibt es die Einrichtung noch nicht; der Aufrufer übergibt dann
/// `false`.
///
/// `plaintext_database`: Bei einer vorhandenen Klartext-Datenbank kostet ein
/// neuer Schlüssel den bisherigen Verlauf — A3 verlangt für diesen Fall
/// denselben Hinweis wie in D4.
pub fn d1_keychain_unreachable_text(
    reason: KeychainUnavailableReason,
    target_os: &str,
    offers_password_setup: bool,
    plaintext_database: bool,
    language: Language,
) -> ChoiceDialogText {
    let cause = keychain_unavailable_text(reason, target_os, language);
    let (untouched, retry, setup) = match language {
        Language::De => (
            "An deinen Daten wurde nichts verändert. Smart SSH hat die Datenbank nicht geöffnet.",
            "Erneut versuchen",
            "Master-Passwort einrichten",
        ),
        Language::En => (
            "Nothing about your data has been changed. Smart SSH has not opened the database.",
            "Try again",
            "Set up a master password",
        ),
    };
    let history_warning = match (offers_password_setup && plaintext_database, language) {
        (true, Language::De) => Some(
            "Richtest du jetzt ein Master-Passwort ein, entsteht dabei ein neuer Schlüssel: \
             Server, Regeln und Einstellungen bleiben, der bisherige Chatverlauf wird \
             unlesbar.",
        ),
        (true, Language::En) => Some(
            "Setting up a master password now creates a new key: servers, rules and settings \
             are kept, but the existing chat history becomes unreadable.",
        ),
        (false, _) => None,
    };
    let mut message = format!("{}\n\n{untouched}", cause.message);
    if let Some(warning) = history_warning {
        message.push_str("\n\n");
        message.push_str(warning);
    }
    ChoiceDialogText {
        title: cause.title,
        message,
        confirm: retry.to_string(),
        cancel: quit_label(language),
        extra: offers_password_setup.then(|| setup.to_string()),
    }
}

/// Spec 0101, D2: Die Datei ist mit dem vorhandenen Schlüssel nicht lesbar
/// (oder es gibt keinen Schlüssel zu einer verschlüsselten Datei).
///
/// **Kein „Backup einspielen“ als erste Wahl** (D2, wörtlich): Ein Backup
/// der verschlüsselten Datei braucht denselben Schlüssel und wäre genauso
/// unlesbar. Dieser Rat wäre also nicht nur unnütz, sondern würde Zeit in
/// einen Weg stecken, der nicht hilft.
pub fn d2_not_readable_text(db_path: &Path, language: Language) -> ChoiceDialogText {
    let db_path = sanitize_path_for_display(db_path);
    let (message, confirm) = match language {
        Language::De => (
            format!(
                "Die Datenbank von Smart SSH lässt sich mit dem vorhandenen Schlüssel nicht \
                 lesen. Entweder ist die Datei beschädigt, oder sie gehört zu einem anderen \
                 Schlüssel.\n\n\
                 Du kannst neu anfangen: Die bisherige Datei wird dabei umbenannt und bleibt \
                 liegen — gelöscht wird nichts. Smart SSH startet dann mit einer leeren \
                 Datenbank.\n\n\
                 Datenpfad: {db_path}"
            ),
            "Neu anfangen",
        ),
        Language::En => (
            format!(
                "Smart SSH's database cannot be read with the available key. The file is \
                 either damaged or belongs to a different key.\n\n\
                 You can start over: the existing file is renamed and kept — nothing is \
                 deleted. Smart SSH then starts with an empty database.\n\n\
                 Data path: {db_path}"
            ),
            "Start over",
        ),
    };
    ChoiceDialogText {
        title: cannot_start_title(language).to_string(),
        message,
        confirm: confirm.to_string(),
        cancel: quit_label(language),
        extra: None,
    }
}

/// Spec 0101, D3: „Schlüssel unbrauchbar“. Der vorhandene Eintrag wird
/// **erst nach dieser Wahl** ersetzt — das steht im Text, weil es den
/// Unterschied macht zwischen „ich kann noch etwas versuchen“ und „es ist
/// entschieden".
pub fn d3_unusable_key_text(db_path: &Path, language: Language) -> ChoiceDialogText {
    let db_path = sanitize_path_for_display(db_path);
    let (title, message, confirm) = match language {
        Language::De => (
            "Schlüssel unbrauchbar",
            format!(
                "Der im Schlüsselbund hinterlegte Schlüssel von Smart SSH ist kein gültiger \
                 Schlüssel. Ohne ihn lässt sich die Datenbank nicht öffnen.\n\n\
                 Bis du hier wählst, wird nichts verändert — weder der Eintrag im \
                 Schlüsselbund noch die Datenbank. Mit „Neu anfangen“ wird ein neuer \
                 Schlüssel erzeugt, die bisherige Datei umbenannt (nicht gelöscht), und \
                 Smart SSH startet leer.\n\n\
                 Datenpfad: {db_path}"
            ),
            "Neu anfangen",
        ),
        Language::En => (
            "Unusable key",
            format!(
                "The key stored for Smart SSH in the keychain is not a valid key. Without it \
                 the database cannot be opened.\n\n\
                 Nothing is changed until you choose here — neither the keychain entry nor \
                 the database. Start over creates a new key, renames the existing file (it \
                 is not deleted), and starts Smart SSH empty.\n\n\
                 Data path: {db_path}"
            ),
            "Start over",
        ),
    };
    ChoiceDialogText {
        title: title.to_string(),
        message,
        confirm: confirm.to_string(),
        cancel: quit_label(language),
        extra: None,
    }
}

/// Spec 0101, D4: wie D3, aber die Klartext-Datei bleibt lesbar — es geht
/// **nur** der feldweise verschlüsselte Verlauf verloren, und der Text
/// benennt genau das, statt pauschal vor Datenverlust zu warnen.
pub fn d4_new_key_for_plaintext_text(db_path: &Path, language: Language) -> ChoiceDialogText {
    let db_path = sanitize_path_for_display(db_path);
    let (title, message, confirm) = match language {
        Language::De => (
            "Schlüssel unbrauchbar",
            format!(
                "Der im Schlüsselbund hinterlegte Schlüssel von Smart SSH ist kein gültiger \
                 Schlüssel. Deine Datenbank ist noch unverschlüsselt und damit lesbar.\n\n\
                 Mit einem neuen Schlüssel bleiben Server, Gruppen, Regeln und Einstellungen \
                 vollständig erhalten. Verloren geht allein der bisherige Chatverlauf \
                 (einschließlich Ausführungsprotokoll und Eingabe-Historie) — er war mit dem \
                 unbrauchbaren Schlüssel verschlüsselt.\n\n\
                 Bis du hier wählst, wird nichts verändert.\n\n\
                 Datenpfad: {db_path}"
            ),
            "Neuen Schlüssel erzeugen",
        ),
        Language::En => (
            "Unusable key",
            format!(
                "The key stored for Smart SSH in the keychain is not a valid key. Your \
                 database is still unencrypted and therefore readable.\n\n\
                 With a new key, servers, groups, rules and settings are kept in full. The \
                 only thing lost is the existing chat history (including the execution \
                 ledger and the input history) — it was encrypted with the unusable key.\n\n\
                 Nothing is changed until you choose here.\n\n\
                 Data path: {db_path}"
            ),
            "Create a new key",
        ),
    };
    ChoiceDialogText {
        title: title.to_string(),
        message,
        confirm: confirm.to_string(),
        cancel: quit_label(language),
        extra: None,
    }
}

/// Spec 0101, A5: die **zweite** Bestätigung für „Neu anfangen“ — mit dem
/// Namen, den die bisherige Datei bekommt.
pub fn start_over_confirmation_text(renamed_to: &str, language: Language) -> ChoiceDialogText {
    let renamed_to = sanitize_text_for_display(renamed_to);
    let (title, message, confirm) = match language {
        Language::De => (
            "Wirklich neu anfangen?",
            format!(
                "Smart SSH startet danach mit einer leeren Datenbank. Server, Gruppen, \
                 Regeln, Chatverlauf und Einstellungen sind dann nicht mehr sichtbar.\n\n\
                 Gelöscht wird nichts: Die bisherige Datei bleibt als \"{renamed_to}\" im \
                 Datenverzeichnis liegen."
            ),
            "Ja, neu anfangen",
        ),
        Language::En => (
            "Really start over?",
            format!(
                "Smart SSH will then start with an empty database. Servers, groups, rules, \
                 chat history and settings will no longer be visible.\n\n\
                 Nothing is deleted: the existing file is kept as \"{renamed_to}\" in the \
                 data directory."
            ),
            "Yes, start over",
        ),
    };
    ChoiceDialogText {
        title: title.to_string(),
        message,
        confirm: confirm.to_string(),
        cancel: cancel_label(language),
        extra: None,
    }
}

/// Spec 0101, D4 („Zweite Bestätigung wie A5“).
pub fn new_key_confirmation_text(language: Language) -> ChoiceDialogText {
    let (title, message, confirm) = match language {
        Language::De => (
            "Wirklich einen neuen Schlüssel erzeugen?",
            "Der bisherige Chatverlauf, das Ausführungsprotokoll und die Eingabe-Historie \
             werden damit unlesbar. Server, Gruppen, Regeln und Einstellungen bleiben \
             erhalten.",
            "Ja, neuen Schlüssel erzeugen",
        ),
        Language::En => (
            "Really create a new key?",
            "The existing chat history, execution ledger and input history will become \
             unreadable. Servers, groups, rules and settings are kept.",
            "Yes, create a new key",
        ),
    };
    ChoiceDialogText {
        title: title.to_string(),
        message: message.to_string(),
        confirm: confirm.to_string(),
        cancel: cancel_label(language),
        extra: None,
    }
}

/// Spec 0101, A5: „nennt im Dialog den neuen Dateinamen“ — nach dem
/// Umbenennen, damit der Nutzer die Datei wiederfindet.
pub fn started_over_notice_text(renamed_to: &str, language: Language) -> DialogText {
    let renamed_to = sanitize_text_for_display(renamed_to);
    let (title, message) = match language {
        Language::De => (
            "Smart SSH startet neu",
            format!(
                "Die bisherige Datenbank liegt jetzt als \"{renamed_to}\" im \
                 Datenverzeichnis. Sie wurde nicht gelöscht."
            ),
        ),
        Language::En => (
            "Smart SSH is starting fresh",
            format!(
                "The previous database is now stored as \"{renamed_to}\" in the data \
                 directory. It has not been deleted."
            ),
        ),
    };
    DialogText {
        title: title.to_string(),
        message,
    }
}

/// Wie `sanitize_path_for_display`, für einen Dateinamen — derselbe Grund
/// (Spec 0071, X1): Ein Steuerzeichen darin könnte den Dialogtext optisch
/// fortsetzen.
fn sanitize_text_for_display(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DB: &str = "/tmp/smart-ssh/smart-ssh.db";

    /// A20: Jeder dieser Dialoge hat in **beiden** Sprachen Titel, Text und
    /// zwei beschriftete Knöpfe — ein leerer Knopf wäre im nativen Dialog
    /// eine unbeschriftete Fläche.
    #[test]
    fn test_a20_every_choice_dialog_is_complete_in_both_languages() {
        for language in [Language::De, Language::En] {
            let path = Path::new(DB);
            let dialogs = [
                d1_keychain_unreachable_text(
                    KeychainUnavailableReason::Locked,
                    "macos",
                    false,
                    false,
                    language,
                ),
                d2_not_readable_text(path, language),
                d3_unusable_key_text(path, language),
                d4_new_key_for_plaintext_text(path, language),
                start_over_confirmation_text("smart-ssh.db.unreadable-20261002T120000Z", language),
                new_key_confirmation_text(language),
            ];
            for dialog in dialogs {
                assert!(!dialog.title.trim().is_empty(), "{dialog:?}");
                assert!(!dialog.message.trim().is_empty(), "{dialog:?}");
                assert!(!dialog.confirm.trim().is_empty(), "{dialog:?}");
                assert!(!dialog.cancel.trim().is_empty(), "{dialog:?}");
            }
            let notice = started_over_notice_text("smart-ssh.db.unreadable-x", language);
            assert!(!notice.title.trim().is_empty());
            assert!(!notice.message.trim().is_empty());
        }
    }

    /// D2, wörtlich: **kein** Hinweis auf „Backup einspielen“ als erste
    /// Wahl. Der `Other`-Text aus Spec 0059 enthält ihn — dieser Test hält
    /// fest, dass D2 nicht auf ihn zurückfällt.
    #[test]
    fn test_d2_does_not_advise_restoring_a_backup() {
        for language in [Language::De, Language::En] {
            let text = d2_not_readable_text(Path::new(DB), language);
            let lower = text.message.to_lowercase();
            assert!(
                !lower.contains("backup"),
                "D2 darf kein Backup empfehlen ({language:?}): {}",
                text.message
            );
        }
    }

    /// A3, D1: Der dritte Knopf erscheint genau dann, wenn der Aufrufer ihn
    /// erlaubt — und der Hinweis auf den Verlust des Verlaufs nur bei einer
    /// vorhandenen Klartext-Datenbank (D1, letzter Satz).
    #[test]
    fn test_d1_third_button_and_history_warning_appear_only_when_asked() {
        let without = d1_keychain_unreachable_text(
            KeychainUnavailableReason::NoSessionBus,
            "linux",
            false,
            true,
            Language::De,
        );
        assert_eq!(without.extra, None);
        assert!(!without.message.contains("Chatverlauf"));

        let with_setup_fresh = d1_keychain_unreachable_text(
            KeychainUnavailableReason::NoSessionBus,
            "linux",
            true,
            false,
            Language::De,
        );
        assert!(with_setup_fresh.extra.is_some());
        assert!(
            !with_setup_fresh.message.contains("Chatverlauf"),
            "ohne vorhandene Klartext-Datenbank gibt es keinen Verlauf zu verlieren"
        );

        let with_setup_plaintext = d1_keychain_unreachable_text(
            KeychainUnavailableReason::NoSessionBus,
            "linux",
            true,
            true,
            Language::De,
        );
        assert!(with_setup_plaintext.extra.is_some());
        assert!(with_setup_plaintext.message.contains("Chatverlauf"));
    }

    /// Spec 0071, X1: Ein Steuerzeichen im Dateinamen darf den Dialogtext
    /// nicht optisch fortsetzen.
    #[test]
    fn test_control_characters_in_a_file_name_cannot_continue_the_text() {
        let text = start_over_confirmation_text(
            "smart-ssh.db.unreadable-x\n\nAlles gelöscht!",
            Language::De,
        );
        assert!(
            !text.message.contains("\n\nAlles gelöscht!"),
            "Steuerzeichen müssen ersetzt sein: {}",
            text.message
        );
        assert!(text.message.contains("??Alles gelöscht!"));
    }
}
