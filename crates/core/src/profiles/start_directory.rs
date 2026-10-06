//! Spec 0102: optionales Startverzeichnis je Server für das interaktive
//! Terminal und den SFTP-Dateibrowser.
//!
//! Reine Hilfsfunktionen ohne I/O: Prüfen/Bereinigen des eingegebenen
//! Werts, Abbilden auf einen SFTP-Pfad und Bauen der sichtbaren
//! `cd`-Zeile fürs Terminal. Bewusst **keine** Sicherheitsgrenze: Terminal
//! und SFTP laufen ohnehin nicht durch die Filter-Engine (s.
//! `crate::ssh::SftpSession`/`InteractiveShell`), und KI-Kommandos sehen
//! diesen Wert nie.

use std::fmt;

/// Präfix für Pfade relativ zum Home-Verzeichnis des Login-Nutzers.
const HOME_PREFIX: &str = "~/";

/// Warum ein eingegebenes Startverzeichnis abgelehnt wurde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartDirectoryError {
    /// Weder absolut (`/…`) noch relativ zum Home (`~/…`).
    NotAbsolute,
    /// Enthält ein Steuerzeichen (z. B. Zeilenumbruch). Ein solches Zeichen
    /// würde die ins Terminal geschriebene `cd`-Zeile vorzeitig abschicken
    /// oder zerreißen.
    ControlCharacter,
}

impl fmt::Display for StartDirectoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartDirectoryError::NotAbsolute => f.write_str(
                "Das Startverzeichnis muss ein absoluter Pfad (/…) sein oder mit ~/ beginnen",
            ),
            StartDirectoryError::ControlCharacter => {
                f.write_str("Das Startverzeichnis darf keine Steuerzeichen enthalten")
            }
        }
    }
}

impl std::error::Error for StartDirectoryError {}

/// Prüft und bereinigt ein eingegebenes Startverzeichnis.
///
/// - Leerraum am Rand wird entfernt; ein danach leerer Wert heißt „nicht
///   gesetzt" (`Ok(None)`).
/// - Erlaubt sind absolute Pfade (`/…`) und Pfade, die mit `~/` beginnen.
///   `~` allein, `~nutzer/…` und alle anderen relativen Pfade werden
///   abgelehnt.
/// - Steuerzeichen werden abgelehnt (s. [`StartDirectoryError::ControlCharacter`]).
pub fn normalize_start_directory(input: &str) -> Result<Option<String>, StartDirectoryError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().any(char::is_control) {
        return Err(StartDirectoryError::ControlCharacter);
    }
    if !(trimmed.starts_with('/') || trimmed.starts_with(HOME_PREFIX)) {
        return Err(StartDirectoryError::NotAbsolute);
    }
    Ok(Some(trimmed.to_string()))
}

/// Der Pfad, unter dem der SFTP-Dateibrowser das Startverzeichnis öffnet.
///
/// SFTP löst relative Pfade gegen das Home-Verzeichnis des Login-Nutzers
/// auf, kennt aber kein `~`. `~/x` wird deshalb zu `./x` — dieselbe Form,
/// in der der Dateibrowser schon heute Unterordner von `"."` (Home) führt,
/// sodass „Aufwärts" von dort wieder bei `"."` landet. `~/` allein ist das
/// Home selbst (`"."`). Absolute Pfade bleiben unverändert.
pub fn start_directory_sftp_path(dir: &str) -> String {
    match dir.strip_prefix(HOME_PREFIX) {
        Some(rest) => {
            let rest = rest.trim_end_matches('/');
            if rest.is_empty() {
                ".".to_string()
            } else {
                format!("./{rest}")
            }
        }
        None => dir.to_string(),
    }
}

/// Die Zeile, die nach dem Start der Login-Shell ins PTY geschrieben wird
/// (Issue-Entscheidung 1): sichtbar für den Nutzer, funktioniert mit jeder
/// gängigen Login-Shell, ohne die Shell-Anfrage selbst zu verändern.
///
/// Der Pfad steht in einfachen Anführungszeichen; ein `'` darin wird als
/// `'\''` geschrieben (Anführung schließen, maskiertes `'`, neu öffnen).
/// Für `~/…` bleibt nur das `~/` außerhalb der Anführung, damit die Shell
/// die Tilde expandiert; der Rest ist ebenso gequotet. Abgeschlossen mit
/// `\r` — dasselbe Byte, das das Terminal für die Eingabetaste sendet.
pub fn start_directory_cd_command(dir: &str) -> String {
    let target = match dir.strip_prefix(HOME_PREFIX) {
        Some("") => "~".to_string(),
        Some(rest) => format!("~/{}", single_quote(rest)),
        None => single_quote(dir),
    };
    format!("cd -- {target}\r")
}

fn single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_whitespace_mean_not_set() {
        assert_eq!(normalize_start_directory(""), Ok(None));
        assert_eq!(normalize_start_directory("   \t "), Ok(None));
    }

    #[test]
    fn absolute_and_home_relative_paths_are_accepted_and_trimmed() {
        assert_eq!(
            normalize_start_directory("  /srv/app  "),
            Ok(Some("/srv/app".to_string()))
        );
        assert_eq!(normalize_start_directory("/"), Ok(Some("/".to_string())));
        assert_eq!(
            normalize_start_directory("~/projects/my app"),
            Ok(Some("~/projects/my app".to_string()))
        );
        assert_eq!(normalize_start_directory("~/"), Ok(Some("~/".to_string())));
    }

    #[test]
    fn relative_paths_are_rejected() {
        for bad in [
            "srv/app",
            "./app",
            "../app",
            "~",
            "~root/app",
            "~app",
            "app",
        ] {
            assert_eq!(
                normalize_start_directory(bad),
                Err(StartDirectoryError::NotAbsolute),
                "{bad}"
            );
        }
    }

    #[test]
    fn control_characters_are_rejected() {
        for bad in ["/srv/a\nb", "/srv/a\rb", "/srv/\u{1b}[31m", "~/a\u{0}b"] {
            assert_eq!(
                normalize_start_directory(bad),
                Err(StartDirectoryError::ControlCharacter),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn sftp_path_maps_home_prefix_to_relative_form() {
        assert_eq!(start_directory_sftp_path("/srv/app"), "/srv/app");
        assert_eq!(start_directory_sftp_path("/"), "/");
        assert_eq!(start_directory_sftp_path("~/"), ".");
        assert_eq!(start_directory_sftp_path("~/projects"), "./projects");
        assert_eq!(start_directory_sftp_path("~/projects/"), "./projects");
        assert_eq!(start_directory_sftp_path("~/a b/c"), "./a b/c");
    }

    #[test]
    fn cd_command_quotes_absolute_paths() {
        assert_eq!(start_directory_cd_command("/srv/app"), "cd -- '/srv/app'\r");
        assert_eq!(
            start_directory_cd_command("/srv/my app"),
            "cd -- '/srv/my app'\r"
        );
    }

    #[test]
    fn cd_command_escapes_single_quotes() {
        assert_eq!(
            start_directory_cd_command("/srv/it's here"),
            "cd -- '/srv/it'\\''s here'\r"
        );
    }

    #[test]
    fn cd_command_keeps_tilde_unquoted_for_home_relative_paths() {
        assert_eq!(start_directory_cd_command("~/"), "cd -- ~\r");
        assert_eq!(
            start_directory_cd_command("~/my 'dir'"),
            "cd -- ~/'my '\\''dir'\\'''\r"
        );
    }

    #[test]
    fn cd_command_neutralises_shell_metacharacters() {
        // Alles zwischen den Anführungszeichen ist für die Shell Literal —
        // kein `$()`, kein `;`, kein Glob wird ausgewertet.
        let cmd = start_directory_cd_command("/tmp/$(id); rm -rf *");
        assert_eq!(cmd, "cd -- '/tmp/$(id); rm -rf *'\r");
    }
}
