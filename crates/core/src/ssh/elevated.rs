//! Spec 0067, Teil A: reine Logik für den Dateibrowser mit erhöhten Rechten.
//! Der SFTP-Server wird per `sudo -n <sftp-server>` über einen Exec-Kanal
//! gestartet (Weg A, wie WinSCP). Nur passwortloses sudo — ein Passwort wird
//! nie verarbeitet. Hier nur Kommando-Bau, Pfad-/Nutzer-Validierung und
//! Auswertung der Probe-Ausgaben; die Kanal-Erzeugung lebt im Transport.

use super::CommandOutput;

/// Ziel-Nutzer, wenn keiner angegeben ist.
pub const DEFAULT_ELEVATION_USER: &str = "root";

/// Übliche Orte von `sftp-server` je Distribution — geprüft in dieser
/// Reihenfolge, nachdem ein `Subsystem sftp`-Eintrag aus `sshd_config`
/// (falls absolut und ausführbar) Vorrang hatte.
pub const KNOWN_SFTP_SERVER_PATHS: &[&str] = &[
    "/usr/lib/openssh/sftp-server",
    "/usr/libexec/openssh/sftp-server",
    "/usr/lib/ssh/sftp-server",
    "/usr/libexec/sftp-server",
    "/usr/lib/sftp-server",
    "/usr/libexec/ssh/sftp-server",
    "/usr/lib64/ssh/sftp-server",
    "/usr/local/libexec/sftp-server",
];

/// Absoluter Pfad aus einem eng begrenzten Zeichensatz. Der Pfad landet
/// ungequotet in einem Shell-Kommando (`sudo -n <pfad>`) und in der
/// sudoers-Zeile — alles außerhalb dieses Zeichensatzes wird abgelehnt statt
/// escapet.
pub fn is_safe_absolute_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 4096
        && !path.contains("..")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '+'))
}

/// Sicherer absoluter Pfad, dessen Dateiname `sftp-server` ist — schützt
/// davor, per Override z. B. `/bin/sh` mit sudo zu starten (und dafür eine
/// NOPASSWD-Regel angezeigt zu bekommen).
pub fn is_plausible_sftp_server_path(path: &str) -> bool {
    is_safe_absolute_path(path) && path.rsplit('/').next() == Some("sftp-server")
}

/// POSIX-übliche Nutzernamen (`[a-z_][a-z0-9_-]*`, max. 32 Zeichen).
pub fn is_valid_target_user(user: &str) -> bool {
    let mut chars = user.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    user.len() <= 32
        && (first.is_ascii_lowercase() || first == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Probe-Kommando zur Pfad-Erkennung: gibt den ersten ausführbaren Pfad aus
/// (erst `sshd_config`, dann [`KNOWN_SFTP_SERVER_PATHS`]), Exit 3 wenn keiner.
pub fn sftp_server_probe_command() -> String {
    let mut cmd = String::from(
        "p=$(awk '$1==\"Subsystem\" && $2==\"sftp\" {print $3; exit}' /etc/ssh/sshd_config 2>/dev/null); \
         case \"$p\" in /*) if [ -x \"$p\" ]; then echo \"$p\"; exit 0; fi;; esac; for p in",
    );
    for path in KNOWN_SFTP_SERVER_PATHS {
        cmd.push(' ');
        cmd.push_str(path);
    }
    cmd.push_str("; do if [ -x \"$p\" ]; then echo \"$p\"; exit 0; fi; done; exit 3");
    cmd
}

/// Liest das Ergebnis von [`sftp_server_probe_command`]. `None`, wenn kein
/// Pfad gefunden wurde oder die Ausgabe kein sicherer absoluter Pfad ist
/// (die Ausgabe stammt vom Server und wird deshalb validiert).
pub fn parse_sftp_server_probe(output: &CommandOutput) -> Option<String> {
    if output.exit_code != Some(0) {
        return None;
    }
    let first_line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    is_plausible_sftp_server_path(&first_line).then_some(first_line)
}

/// `-u <nutzer>` nur, wenn nicht root — die Standard-sudoers-Regel für root
/// braucht kein `-u`, und `sudo -n <pfad>` ist die in der Spec genannte Form.
fn user_args(user: &str) -> String {
    if user == DEFAULT_ELEVATION_USER {
        String::new()
    } else {
        format!(" -u {user}")
    }
}

/// Fehler beim Bau der Kommandos: Pfad oder Nutzer außerhalb des erlaubten
/// Zeichensatzes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevationInputError {
    InvalidPath,
    InvalidUser,
}

fn validate(path: &str, user: &str) -> Result<(), ElevationInputError> {
    if !is_plausible_sftp_server_path(path) {
        return Err(ElevationInputError::InvalidPath);
    }
    if !is_valid_target_user(user) {
        return Err(ElevationInputError::InvalidUser);
    }
    Ok(())
}

/// `sudo -n [-u <nutzer>] -l <pfad>`: prüft ohne Passwort, ob genau dieser
/// Befehl erlaubt ist (Spec 0067, A3). `LC_ALL=C`, damit die Fehlertexte für
/// [`classify_sudo_check`] englisch und damit erkennbar sind.
pub fn sudo_check_command(path: &str, user: &str) -> Result<String, ElevationInputError> {
    validate(path, user)?;
    // `env` statt `VAR=… cmd`: funktioniert auch, wenn die Login-Shell kein
    // POSIX-sh ist (csh/tcsh/fish).
    Ok(format!("env LC_ALL=C sudo -n{} -l {path}", user_args(user)))
}

/// Das Exec-Kommando für den erhöhten SFTP-Kanal: `sudo -n [-u <nutzer>]
/// <pfad>`. `-n`: verlangt sudo ein Passwort, scheitert es sofort.
pub fn elevated_sftp_command(path: &str, user: &str) -> Result<String, ElevationInputError> {
    validate(path, user)?;
    Ok(format!("sudo -n{} {path}", user_args(user)))
}

/// Die zugeschnittene sudoers-Zeile zum Kopieren (Spec 0067, A3). Das
/// abschließende `""` erlaubt `sftp-server` nur ohne Argumente — genau so
/// startet die App ihn. `None`, wenn der Login Zeichen enthält, die in
/// sudoers eine Sonderbedeutung hätten (dann lieber keine Zeile als eine
/// falsche).
pub fn sudoers_line(login: &str, path: &str, user: &str) -> Option<String> {
    let login_ok = !login.is_empty()
        && login
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    login_ok.then(|| format!("{login} ALL=({user}) NOPASSWD: {path} \"\""))
}

/// Ergebnis der Voraussetzungs-Prüfung (`sudo -n -l <pfad>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SudoCheck {
    Allowed,
    /// sudo verlangt ein Passwort — es gibt keine passende NOPASSWD-Regel.
    PasswordRequired,
    /// sudo läuft, aber dieser Befehl ist für den Login nicht erlaubt.
    NotAllowed,
    /// `requiretty` in sudoers: sudo verweigert ohne Terminal.
    RequireTty,
    /// sudo ist auf dem Server nicht installiert.
    SudoMissing,
    /// Unbekannter Fehler — erste stderr-Zeile, gekürzt. Nur für die
    /// Anzeige im Browser, nie für den Chat/KI-Kontext.
    Failed(String),
}

pub fn classify_sudo_check(output: &CommandOutput) -> SudoCheck {
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    if stderr.contains("must have a tty") || stderr.contains("requiretty") {
        return SudoCheck::RequireTty;
    }
    if output.exit_code == Some(127)
        || stderr.contains("sudo: not found")
        || stderr.contains("sudo: command not found")
    {
        return SudoCheck::SudoMissing;
    }
    if stderr.contains("password is required") {
        return SudoCheck::PasswordRequired;
    }
    if stderr.contains("not allowed") || stderr.contains("may not run") {
        return SudoCheck::NotAllowed;
    }
    if output.exit_code == Some(0) {
        return SudoCheck::Allowed;
    }
    // Exit != 0 ohne erkennbaren Grund: `sudo -l <befehl>` meldet "nicht
    // erlaubt" je nach Version auch nur per Exit-Code.
    let first_line = String::from_utf8_lossy(&output.stderr)
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .chars()
        .take(200)
        .collect::<String>();
    if first_line.is_empty() {
        SudoCheck::NotAllowed
    } else {
        SudoCheck::Failed(first_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(exit: i32, stdout: &str, stderr: &str) -> CommandOutput {
        CommandOutput {
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
            exit_code: Some(exit),
            truncated: false,
        }
    }

    #[test]
    fn test_elevated_command_uses_sudo_non_interactive() {
        assert_eq!(
            elevated_sftp_command("/usr/lib/openssh/sftp-server", "root").unwrap(),
            "sudo -n /usr/lib/openssh/sftp-server"
        );
        assert_eq!(
            elevated_sftp_command("/usr/lib/openssh/sftp-server", "www-data").unwrap(),
            "sudo -n -u www-data /usr/lib/openssh/sftp-server"
        );
    }

    #[test]
    fn test_sudo_check_command_lists_exactly_this_path() {
        assert_eq!(
            sudo_check_command("/usr/libexec/openssh/sftp-server", "root").unwrap(),
            "env LC_ALL=C sudo -n -l /usr/libexec/openssh/sftp-server"
        );
    }

    #[test]
    fn test_unsafe_path_or_user_is_rejected_not_escaped() {
        for bad in [
            "sftp-server",
            "/usr/lib/sftp-server; rm -rf /",
            "/usr/lib/$(id)",
            "/usr/lib/../../bin/sh",
            "/usr/lib/sftp server",
            "/usr/lib/sftp-server\n",
        ] {
            assert!(!is_safe_absolute_path(bad), "{bad:?} muss abgelehnt werden");
            assert_eq!(
                elevated_sftp_command(bad, "root"),
                Err(ElevationInputError::InvalidPath)
            );
        }
        for bad in ["", "Root", "a b", "x;id", "-u", "$(id)", &"a".repeat(33)] {
            assert!(!is_valid_target_user(bad), "{bad:?} muss abgelehnt werden");
            assert_eq!(
                elevated_sftp_command("/usr/lib/openssh/sftp-server", bad),
                Err(ElevationInputError::InvalidUser)
            );
        }
    }

    #[test]
    fn test_only_paths_named_sftp_server_are_accepted() {
        assert!(is_plausible_sftp_server_path("/opt/ssh/sftp-server"));
        for bad in [
            "/bin/sh",
            "/bin/bash",
            "/usr/lib/openssh/sftp-server2",
            "/usr/lib/openssh/",
        ] {
            assert!(!is_plausible_sftp_server_path(bad), "{bad}");
            assert_eq!(
                elevated_sftp_command(bad, "root"),
                Err(ElevationInputError::InvalidPath)
            );
        }
    }

    #[test]
    fn test_sudoers_line_is_tailored_to_login_path_and_user() {
        assert_eq!(
            sudoers_line("stefan", "/usr/lib/openssh/sftp-server", "root").as_deref(),
            Some("stefan ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server \"\"")
        );
        assert_eq!(
            sudoers_line("deploy", "/usr/libexec/sftp-server", "www-data").as_deref(),
            Some("deploy ALL=(www-data) NOPASSWD: /usr/libexec/sftp-server \"\"")
        );
        // Sonderzeichen im Login → lieber keine Zeile als eine falsche.
        for login in ["ad\\user", "a b", "x,y", "#1", ""] {
            assert_eq!(
                sudoers_line(login, "/usr/lib/openssh/sftp-server", "root"),
                None
            );
        }
    }

    #[test]
    fn test_probe_command_checks_sshd_config_and_known_paths() {
        let cmd = sftp_server_probe_command();
        assert!(cmd.contains("/etc/ssh/sshd_config"));
        for path in KNOWN_SFTP_SERVER_PATHS {
            assert!(cmd.contains(path));
        }
        assert!(cmd.contains("-x"));
    }

    #[test]
    fn test_parse_probe_accepts_safe_path_and_rejects_everything_else() {
        assert_eq!(
            parse_sftp_server_probe(&out(0, "/usr/lib/openssh/sftp-server\n", "")),
            Some("/usr/lib/openssh/sftp-server".to_string())
        );
        assert_eq!(parse_sftp_server_probe(&out(3, "", "")), None);
        assert_eq!(parse_sftp_server_probe(&out(0, "", "")), None);
        // `internal-sftp` aus sshd_config ist kein ausführbarer Pfad — die
        // Probe würde es gar nicht ausgeben, aber auch eine manipulierte
        // Ausgabe darf nie ungeprüft in ein sudo-Kommando.
        assert_eq!(
            parse_sftp_server_probe(&out(0, "internal-sftp\n", "")),
            None
        );
        assert_eq!(
            parse_sftp_server_probe(&out(0, "/tmp/x; reboot\n", "")),
            None
        );
    }

    #[test]
    fn test_classify_sudo_check_recognises_each_failure() {
        assert_eq!(
            classify_sudo_check(&out(0, "/usr/lib/openssh/sftp-server\n", "")),
            SudoCheck::Allowed
        );
        assert_eq!(
            classify_sudo_check(&out(1, "", "sudo: a password is required\n")),
            SudoCheck::PasswordRequired
        );
        assert_eq!(
            classify_sudo_check(&out(
                1,
                "",
                "sudo: sorry, you must have a tty to run sudo\n"
            )),
            SudoCheck::RequireTty
        );
        assert_eq!(
            classify_sudo_check(&out(127, "", "sh: 1: sudo: not found\n")),
            SudoCheck::SudoMissing
        );
        assert_eq!(
            classify_sudo_check(&out(
                1,
                "",
                "Sorry, user stefan is not allowed to execute '/usr/lib/openssh/sftp-server' as root.\n"
            )),
            SudoCheck::NotAllowed
        );
        assert_eq!(classify_sudo_check(&out(1, "", "")), SudoCheck::NotAllowed);
        assert_eq!(
            classify_sudo_check(&out(1, "", "sudo: unable to resolve host\n")),
            SudoCheck::Failed("sudo: unable to resolve host".to_string())
        );
    }
}
