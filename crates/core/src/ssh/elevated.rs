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

/// Quotet `s` für eine POSIX-Shell (und csh/tcsh/fish) in einfache
/// Anführungszeichen; ein `'` im Text wird zu `'\''`.
pub fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Probe-Kommando zur Pfad-Erkennung: [`posix_sftp_server_probe`] unter
/// `/bin/sh -c`, damit es unabhängig von der Login-Shell (csh/tcsh/fish)
/// läuft. Ausgabe und Exit-Codes wie dort.
pub fn sftp_server_probe_command() -> String {
    format!(
        "/bin/sh -c {}",
        shell_single_quote(&posix_sftp_server_probe())
    )
}

/// Der Probe-Text in POSIX-sh-Syntax: gibt den ersten ausführbaren Pfad aus
/// (erst `sshd_config`, dann [`KNOWN_SFTP_SERVER_PATHS`]), Exit 3 wenn keiner.
pub fn posix_sftp_server_probe() -> String {
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

/// Ergebnis der Eigentümer-/Rechte-Prüfung von `sftp-server` (Spec 0067, A3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SftpServerSafety {
    /// Datei und jedes übergeordnete Verzeichnis gehören root und sind weder
    /// für die Gruppe noch für andere beschreibbar.
    Safe,
    /// Mindestens ein Eintrag ist unsicher — oder die Prüfung selbst ist
    /// gescheitert (fail closed). `path` ist der erste auffällige Eintrag
    /// bzw. der geprüfte Pfad, wenn die Prüfung kein verwertbares Ergebnis
    /// lieferte.
    Unsafe { path: String },
}

/// Probe-Kommando: [`posix_sftp_server_safety_probe`] unter `/bin/sh -c`.
/// `None`, wenn `path` kein plausibler `sftp-server`-Pfad ist (er landet im
/// Kommando).
pub fn sftp_server_safety_probe_command(path: &str) -> Option<String> {
    is_plausible_sftp_server_path(path).then(|| {
        format!(
            "/bin/sh -c {}",
            shell_single_quote(&posix_sftp_server_safety_probe(path))
        )
    })
}

/// Der Prüf-Text in POSIX-sh-Syntax (kein GNU-`stat`): löst den Pfad
/// Komponente für Komponente auf (`readlink`, höchstens 20 Links). Jeden
/// dabei durchlaufenen Symlink prüft er selbst (ohne ihn zu verfolgen) und
/// das Verzeichnis, das ihn enthält, samt allen Eltern bis `/`; am Ende
/// folgen Datei und alle Elternverzeichnisse. Geprüft wird jeweils mit
/// `find -prune`: root gehört der Eintrag und weder Gruppe noch andere dürfen
/// schreiben. Bei einem Symlink zählt nur der Eigentümer: seine Rechtebits
/// sind (Linux: immer `rwxrwxrwx`) ohne Bedeutung, ob ihn jemand ersetzen
/// kann, entscheidet das Verzeichnis, das ihn enthält. Ausgabe `SAFE` oder `UNSAFE <erster auffälliger Pfad>` (Link
/// bzw. Verzeichnis, nicht der Ziel-Pfad des Links); jeder Fehler endet mit
/// Exit 4 (und damit als unsicher).
pub fn posix_sftp_server_safety_probe(path: &str) -> String {
    safety_probe_script(path, "0", None)
}

/// Wie [`posix_sftp_server_safety_probe`], aber mit wählbarer Nutzer-ID für
/// „gehört root“ und optionalem Verzeichnis, an dem der Aufstieg endet (es
/// wird nicht mehr geprüft). Produktiv gilt immer `0` und kein Stopp; die
/// Parameter erlauben Tests ohne root-Rechte.
fn safety_probe_script(path: &str, owner_uid: &str, stop: Option<&str>) -> String {
    let stop_check = stop.map_or_else(String::new, |s| {
        format!("[ \"$e\" = {} ] && return 0; ", shell_single_quote(s))
    });
    format!(
        concat!(
            "chk() {{ o=$(find \"$1\" -prune \\( ! -user {uid} -o -perm -g+w -o -perm -o+w \\) -print) || exit 4; ",
            "if [ -n \"$o\" ]; then echo \"UNSAFE $1\"; exit 0; fi; }}; ",
            "chklink() {{ o=$(find \"$1\" -prune ! -user {uid} -print) || exit 4; ",
            "if [ -n \"$o\" ]; then echo \"UNSAFE $1\"; exit 0; fi; }}; ",
            "chkdir() {{ e=$1; while :; do {stop}chk \"$e\"; [ \"$e\" = / ] && return 0; e=$(dirname \"$e\"); done; }}; ",
            "cur=/; rest={path}; n=0; ",
            "while [ -n \"$rest\" ]; do ",
            "c=${{rest%%/*}}; case \"$rest\" in */*) rest=${{rest#*/}};; *) rest=;; esac; ",
            "case \"$c\" in ''|.) continue;; ..) cur=$(dirname \"$cur\"); continue;; esac; ",
            "next=${{cur%/}}/$c; ",
            "if [ -L \"$next\" ]; then ",
            "n=$((n+1)); [ \"$n\" -gt 20 ] && exit 4; ",
            "chklink \"$next\"; chkdir \"$cur\"; ",
            "t=$(readlink \"$next\") || exit 4; ",
            "case \"$t\" in /*) cur=/; rest=${{t#/}}/$rest;; *) rest=$t/$rest;; esac; ",
            "else cur=$next; fi; done; ",
            "chkdir \"$cur\"; echo SAFE"
        ),
        uid = owner_uid,
        stop = stop_check,
        path = shell_single_quote(path),
    )
}

/// Liest das Ergebnis von [`sftp_server_safety_probe_command`]. Alles außer
/// Exit 0 mit genau `SAFE` ist unsicher; ein gemeldeter Pfad aus der
/// Server-Ausgabe wird validiert, sonst gilt `checked_path`.
pub fn parse_sftp_server_safety_probe(
    output: &CommandOutput,
    checked_path: &str,
) -> SftpServerSafety {
    let unsafe_at = |path: &str| SftpServerSafety::Unsafe {
        path: path.to_string(),
    };
    if output.exit_code != Some(0) {
        return unsafe_at(checked_path);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    let (Some(first), None) = (lines.next(), lines.next()) else {
        return unsafe_at(checked_path);
    };
    if first == "SAFE" {
        return SftpServerSafety::Safe;
    }
    match first.strip_prefix("UNSAFE ") {
        Some(p) if is_safe_absolute_path(p) || p == "/" => unsafe_at(p),
        _ => unsafe_at(checked_path),
    }
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
        let cmd = posix_sftp_server_probe();
        assert!(cmd.contains("/etc/ssh/sshd_config"));
        for path in KNOWN_SFTP_SERVER_PATHS {
            assert!(cmd.contains(path));
        }
        assert!(cmd.contains("-x"));
    }

    #[test]
    fn test_probe_command_runs_under_bin_sh_with_probe_quoted() {
        let cmd = sftp_server_probe_command();
        let rest = cmd.strip_prefix("/bin/sh -c '").expect("prefix");
        assert!(rest.ends_with('\''));
        // Zurück-Entquoten: `'\''` -> `'`.
        let inner = &rest[..rest.len() - 1];
        assert_eq!(inner.replace("'\\''", "'"), posix_sftp_server_probe());
    }

    #[cfg(unix)]
    #[test]
    fn test_shell_single_quote_round_trips_through_sh() {
        for s in [
            "plain",
            "it's",
            "'",
            "a$b",
            "x;y",
            "$(id)",
            "`id`",
            "a b\"c\\d",
            "''",
            "",
        ] {
            let out = std::process::Command::new("/bin/sh")
                .args(["-c", &format!("printf %s {}", shell_single_quote(s))])
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), s);
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_probe_command_under_real_sh_prints_sshd_config_path() {
        use std::os::unix::fs::PermissionsExt;
        // Der Probe liest /etc/ssh/sshd_config; ist dort kein absoluter,
        // ausführbarer Subsystem-Pfad, würde das Ergebnis vom Host abhängen.
        // Deshalb: Probe-Text mit ersetztem Config-Pfad durch dieselbe
        // Quoting-Funktion und echte `/bin/sh`.
        let dir = std::env::temp_dir().join(format!("probe-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("sftp-server");
        std::fs::write(&fake, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let conf = dir.join("sshd_config");
        std::fs::write(&conf, format!("Subsystem sftp {}\n", fake.display())).unwrap();

        let probe =
            posix_sftp_server_probe().replace("/etc/ssh/sshd_config", &conf.display().to_string());
        let cmd = format!("/bin/sh -c {}", shell_single_quote(&probe));
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", &cmd])
            .output()
            .unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            fake.display().to_string()
        );
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

    const SAFETY_PATH: &str = "/usr/lib/openssh/sftp-server";

    #[test]
    fn test_safety_parser_reports_safe_tree() {
        assert_eq!(
            parse_sftp_server_safety_probe(&out(0, "SAFE\n", ""), SAFETY_PATH),
            SftpServerSafety::Safe
        );
    }

    #[test]
    fn test_safety_parser_reports_each_unsafe_entry_with_its_path() {
        // Datei nicht root-eigen / gruppen- / welt-beschreibbar und
        // welt-beschreibbares Elternverzeichnis: die Probe nennt den Eintrag.
        for entry in [
            "/usr/lib/openssh/sftp-server",
            "/usr/lib/openssh",
            "/usr",
            "/",
        ] {
            assert_eq!(
                parse_sftp_server_safety_probe(
                    &out(0, &format!("UNSAFE {entry}\n"), ""),
                    SAFETY_PATH
                ),
                SftpServerSafety::Unsafe {
                    path: entry.to_string()
                }
            );
        }
    }

    #[test]
    fn test_safety_parser_reports_symlink_target_path() {
        assert_eq!(
            parse_sftp_server_safety_probe(
                &out(0, "UNSAFE /opt/real/sftp-server\n", ""),
                "/usr/lib/openssh/sftp-server"
            ),
            SftpServerSafety::Unsafe {
                path: "/opt/real/sftp-server".to_string()
            }
        );
    }

    #[test]
    fn test_safety_parser_fails_closed_on_garbage_and_failure() {
        let fallback = SftpServerSafety::Unsafe {
            path: SAFETY_PATH.to_string(),
        };
        for o in [
            out(0, "", ""),
            out(0, "garbage\n", ""),
            out(0, "SAFE\nSAFE\n", ""),
            out(0, "SAFE extra\n", ""),
            out(0, "UNSAFE /tmp/x; reboot\n", ""),
            out(0, "UNSAFE relative\n", ""),
            out(4, "", "find: no such file"),
            out(1, "SAFE\n", ""),
            CommandOutput {
                stdout: b"SAFE\n".to_vec(),
                stderr: Vec::new(),
                exit_code: None,
                truncated: false,
            },
        ] {
            assert_eq!(
                parse_sftp_server_safety_probe(&o, SAFETY_PATH),
                fallback,
                "{o:?}"
            );
        }
    }

    #[test]
    fn test_safety_probe_command_validates_path_and_wraps_in_sh() {
        assert_eq!(sftp_server_safety_probe_command("/bin/sh"), None);
        assert_eq!(sftp_server_safety_probe_command("/x; id/sftp-server"), None);
        let cmd = sftp_server_safety_probe_command(SAFETY_PATH).unwrap();
        assert!(cmd.starts_with("/bin/sh -c '"));
        assert!(cmd.contains("-prune"));
        assert!(!cmd.contains("sudo"));
        assert!(!cmd.contains("stat -c"));
    }

    /// Führt die Probe unter `/bin/sh` aus. „Gehört root“ gilt hier für den
    /// aktuellen Nutzer, und der Aufstieg endet am (nicht geprüften)
    /// Testverzeichnis: `/tmp` und Co. sind nie root-eigen und sicher.
    #[cfg(unix)]
    fn run_probe(path: &std::path::Path, stop: &std::path::Path) -> (Option<i32>, String) {
        use std::os::unix::fs::MetadataExt;
        let uid = std::fs::metadata(path.ancestors().find(|a| a.parent() == Some(stop)).unwrap())
            .unwrap()
            .uid()
            .to_string();
        let script = safety_probe_script(
            &path.display().to_string(),
            &uid,
            Some(&stop.display().to_string()),
        );
        let o = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .output()
            .unwrap();
        (
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
        )
    }

    #[cfg(unix)]
    fn mode(p: &std::path::Path, m: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    }

    #[cfg(unix)]
    struct Tree(std::path::PathBuf);
    #[cfg(unix)]
    impl Tree {
        fn new(name: &str) -> Self {
            let d = std::env::temp_dir().join(format!("safety-{name}-{}", std::process::id()));
            std::fs::remove_dir_all(&d).ok();
            std::fs::create_dir_all(&d).unwrap();
            mode(&d, 0o755);
            Tree(std::fs::canonicalize(&d).unwrap())
        }
        fn dir(&self, rel: &str) -> std::path::PathBuf {
            let d = self.0.join(rel);
            std::fs::create_dir_all(&d).unwrap();
            mode(&d, 0o755);
            d
        }
        fn bin(&self, rel: &str) -> std::path::PathBuf {
            let f = self.0.join(rel);
            std::fs::write(&f, "#!/bin/sh\n").unwrap();
            mode(&f, 0o755);
            f
        }
    }
    #[cfg(unix)]
    impl Drop for Tree {
        fn drop(&mut self) {
            // Schreibrechte zurück, damit das Aufräumen nie hängen bleibt.
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    #[cfg(unix)]
    fn unsafe_at(p: &std::path::Path) -> (Option<i32>, String) {
        (Some(0), format!("UNSAFE {}", p.display()))
    }

    #[cfg(unix)]
    #[test]
    fn test_safety_probe_real_sh_plain_tree_safe_and_writable_file_unsafe() {
        let t = Tree::new("plain");
        let real = t.dir("real");
        let bin = t.bin("real/sftp-server");
        let stop = t.0.parent().unwrap().to_path_buf();
        assert_eq!(run_probe(&bin, &stop), (Some(0), "SAFE".to_string()));
        mode(&bin, 0o757);
        assert_eq!(run_probe(&bin, &stop), unsafe_at(&bin));
        mode(&bin, 0o755);
        mode(&real, 0o775);
        assert_eq!(run_probe(&bin, &stop), unsafe_at(&real));
        // Fehlende Datei: Exit != 0 (fail closed durch den Parser).
        assert_ne!(
            run_probe(&real.join("missing/sftp-server"), &stop).0,
            Some(0)
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_safety_probe_real_sh_link_in_writable_dir_is_unsafe_with_link_path() {
        let t = Tree::new("linkdir");
        let bin = t.bin("sftp-server-real");
        let ld = t.dir("links");
        let link = ld.join("sftp-server");
        std::os::unix::fs::symlink(&bin, &link).unwrap();
        let stop = t.0.parent().unwrap().to_path_buf();
        assert_eq!(run_probe(&link, &stop), (Some(0), "SAFE".to_string()));
        mode(&ld, 0o777);
        // Das Ziel ist sicher; gemeldet wird das Verzeichnis des Links.
        assert_eq!(run_probe(&link, &stop), unsafe_at(&ld));
        mode(&ld, 0o770);
        assert_eq!(run_probe(&link, &stop), unsafe_at(&ld));
    }

    #[cfg(unix)]
    #[test]
    fn test_safety_probe_real_sh_link_owned_by_other_user_is_unsafe() {
        let t = Tree::new("owner");
        let bin = t.bin("real");
        let link = t.0.join("sftp-server");
        std::os::unix::fs::symlink(&bin, &link).unwrap();
        // Gegenprobe: „root“ ist hier eine fremde UID → der Link selbst
        // fällt vor Ziel und Eltern auf.
        use std::os::unix::fs::MetadataExt;
        let other = (std::fs::metadata(&bin).unwrap().uid() + 1).to_string();
        let script = safety_probe_script(
            &link.display().to_string(),
            &other,
            Some(&t.0.parent().unwrap().display().to_string()),
        );
        let o = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0));
        assert_eq!(
            String::from_utf8_lossy(&o.stdout).trim(),
            format!("UNSAFE {}", link.display())
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_safety_probe_real_sh_chain_flags_only_intermediate_link_dir() {
        let t = Tree::new("chain");
        let bin = t.bin("target");
        let d1 = t.dir("a");
        let d2 = t.dir("b");
        let d3 = t.dir("c");
        // c/l3 -> ../target ; b/l2 -> ../c/l3 (relativ) ; a/l1 -> b/l2 (relativ)
        std::os::unix::fs::symlink("../target", d3.join("l3")).unwrap();
        std::os::unix::fs::symlink("../c/l3", d2.join("l2")).unwrap();
        std::os::unix::fs::symlink("../b/l2", d1.join("l1")).unwrap();
        let _ = bin;
        let stop = t.0.parent().unwrap().to_path_buf();
        let entry = d1.join("l1");
        assert_eq!(run_probe(&entry, &stop), (Some(0), "SAFE".to_string()));
        mode(&d2, 0o757);
        assert_eq!(run_probe(&entry, &stop), unsafe_at(&d2));
        mode(&d2, 0o755);
        mode(&d3, 0o775);
        assert_eq!(run_probe(&entry, &stop), unsafe_at(&d3));
        mode(&d3, 0o755);
        // Verzeichnis-Symlink als Pfad-Komponente zählt ebenfalls.
        let dl = t.0.join("dirlink");
        std::os::unix::fs::symlink(&d1, &dl).unwrap();
        assert_eq!(
            run_probe(&dl.join("l1"), &stop),
            (Some(0), "SAFE".to_string())
        );
        mode(&t.0, 0o777);
        // Das Verzeichnis, das den Link enthält, ist jetzt beschreibbar.
        assert_eq!(run_probe(&dl.join("l1"), &stop), unsafe_at(&t.0));
    }

    #[cfg(unix)]
    #[test]
    fn test_safety_probe_real_sh_loops_and_depth_limit_fail_closed() {
        let t = Tree::new("loop");
        let stop = t.0.parent().unwrap().to_path_buf();
        let (a, b) = (t.0.join("a"), t.0.join("b"));
        std::os::unix::fs::symlink(&b, &a).unwrap();
        std::os::unix::fs::symlink(&a, &b).unwrap();
        assert_eq!(run_probe(&a, &stop).0, Some(4));
        // 21 Links hintereinander überschreiten die Grenze, 20 nicht.
        let bin = t.bin("target");
        let mut prev = bin;
        for i in 0..21 {
            let l = t.0.join(format!("l{i}"));
            std::os::unix::fs::symlink(&prev, &l).unwrap();
            if i == 19 {
                assert_eq!(run_probe(&l, &stop), (Some(0), "SAFE".to_string()));
            }
            prev = l;
        }
        assert_eq!(run_probe(&prev, &stop).0, Some(4));
    }

    #[cfg(unix)]
    #[test]
    fn test_safety_probe_production_script_flags_non_root_tree() {
        // Produktiv gilt „gehört root“ wörtlich; ein Test-Nutzer ist nie root.
        if std::env::var("USER").as_deref() == Ok("root") {
            return;
        }
        let t = Tree::new("prod");
        let bin = t.bin("sftp-server");
        let script = posix_sftp_server_safety_probe(&bin.display().to_string());
        let o = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&o.stdout).starts_with("UNSAFE "));
    }
}
