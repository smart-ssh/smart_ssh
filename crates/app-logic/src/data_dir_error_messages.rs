//! Startup dialog text for an unusable data directory (issue #1313).

use crate::startup_error_messages::{
    sanitize_path_for_display, sanitize_text_for_display, DialogText, Language,
};

/// Issue #1313: the chosen data directory (flag, environment variable or
/// pointer file) is unusable. Names the path and where it came from, and
/// says that the default data directory was **not** used instead.
pub fn data_dir_failure_text(
    err: &persistence_sqlite::DataDirError,
    language: Language,
) -> DialogText {
    let path = sanitize_path_for_display(&err.path);
    let source = match (language, err.source, &err.pointer_file) {
        (Language::En, persistence_sqlite::DataDirSource::PointerFile, Some(file)) => {
            format!("pointer file {}", sanitize_path_for_display(file))
        }
        (Language::De, persistence_sqlite::DataDirSource::PointerFile, Some(file)) => {
            format!("Zeigerdatei {}", sanitize_path_for_display(file))
        }
        (Language::En, persistence_sqlite::DataDirSource::Flag, _) => {
            "command-line argument --data-dir".to_owned()
        }
        (Language::De, persistence_sqlite::DataDirSource::Flag, _) => {
            "Befehlszeilenargument --data-dir".to_owned()
        }
        (Language::En, _, _) => "environment variable SMART_SSH_DATA_DIR".to_owned(),
        (Language::De, _, _) => "Umgebungsvariable SMART_SSH_DATA_DIR".to_owned(),
    };
    let reason = sanitize_text_for_display(&err.reason);
    match language {
        Language::En => DialogText {
            title: "Smart SSH cannot use the data directory".to_owned(),
            message: format!(
                "The data directory chosen for this instance cannot be used.\n\n\
                 Directory: {path}\nChosen by: {source}\nCause: {reason}\n\n\
                 Smart SSH did not fall back to the default data directory and changed \
                 nothing. Please fix the path or the permissions, or remove the setting."
            ),
        },
        Language::De => DialogText {
            title: "Smart SSH kann das Datenverzeichnis nicht verwenden".to_owned(),
            message: format!(
                "Das für diese Instanz gewählte Datenverzeichnis kann nicht verwendet \
                 werden.\n\nVerzeichnis: {path}\nGewählt über: {source}\nUrsache: {reason}\n\n\
                 Smart SSH ist nicht auf das Standard-Datenverzeichnis ausgewichen und hat \
                 nichts verändert. Bitte Pfad oder Berechtigungen korrigieren oder die \
                 Einstellung entfernen."
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_dir_failure_text_names_path_and_source_in_both_languages() {
        let err = persistence_sqlite::DataDirError {
            path: "/nope/dir".into(),
            source: persistence_sqlite::DataDirSource::PointerFile,
            pointer_file: Some("/apps/Test.data-dir".into()),
            reason: "cannot create it: denied".into(),
        };
        for language in [Language::En, Language::De] {
            let text = data_dir_failure_text(&err, language);
            assert!(text.message.contains("/nope/dir"), "{}", text.message);
            assert!(
                text.message.contains("/apps/Test.data-dir"),
                "{}",
                text.message
            );
            assert!(text.message.contains("denied"));
        }
        let flag = persistence_sqlite::DataDirError {
            source: persistence_sqlite::DataDirSource::Flag,
            pointer_file: None,
            ..err
        };
        assert!(data_dir_failure_text(&flag, Language::En)
            .message
            .contains("--data-dir"));
    }
}
