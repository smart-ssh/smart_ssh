//! Die konkrete Umsetzung von [`KeyFileReader`] (Spec 0076, §4.2) — die
//! **einzige** Stelle, an der Smart SSH eine Schlüsseldatei von der Platte
//! liest.
//!
//! Warum hier und nicht in `core`: Eine Schlüsseldatei zu lesen ist eine
//! äußere Grenze wie `SshTransport`, `CredentialStore` oder `ProfileStore`,
//! und „`core` never depends on Tauri or a UI framework" verbietet den
//! `std::fs::read` mitten in der Domänenlogik (§1.3). `core` kennt nur den
//! Trait und entscheidet, was aus dem Befund folgt (A-5).
//!
//! **Prüfung und Lesen laufen auf demselben offenen Handle** (A-3, E-2,
//! §6.4.7). Ein Vorab-`stat` auf den Pfad wäre ein Wettlauf: Zwischen
//! Prüfung und Lesen könnte die Datei gegen eine andere getauscht werden,
//! und die Rechteprüfung aus E-1 griffe dann nicht auf der Datei, die
//! tatsächlich gelesen wird. Symbolischen Links wird gefolgt (E-2) — genau
//! deshalb ist das Handle die einzige belastbare Grundlage.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use secrecy::SecretString;
use ssh_manager_core::ssh::{
    KeyFileContent, KeyFileError, KeyFileFacts, KeyFileReader, MAX_KEY_FILE_BYTES,
};
use ssh_transport::{classify_openssh_key, KeyClassification};

/// `O_NONBLOCK`, je Plattform.
///
/// A-3 verlangt ihn ausdrücklich („sonst blockiert ein benanntes Rohr, bis
/// ein Schreiber erscheint"). Rusts Standardbibliothek gibt die Konstante
/// nicht heraus; sie kommt hier trotzdem **nicht** aus einer neuen
/// Abhängigkeit (`libc` ist im Arbeitsbereich nirgends direkt eingebunden,
/// und eine Abhängigkeit für eine einzelne Zahl aufzunehmen, wäre nicht
/// verhältnismäßig). Die Werte sind Teil der jeweiligen Kernel-ABI und
/// damit so unveränderlich wie eine Syscall-Nummer.
///
/// **Der Wert wird nicht behauptet, sondern gemessen:**
/// `test_named_pipe_is_rejected_without_blocking` öffnet ein benanntes Rohr
/// ohne Schreiber unter einem Timeout — stimmte die Konstante nicht, hinge
/// der Test, statt grün zu werden.
#[cfg(any(target_os = "linux", target_os = "android"))]
const O_NONBLOCK: i32 = 0o4000;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
const O_NONBLOCK: i32 = 0x0004;

/// Liest Schlüsseldateien vom lokalen Dateisystem (Spec 0076).
pub struct OsKeyFileReader {
    /// Nur für Tests belegt: `~` löst sonst über
    /// [`directories::BaseDirs`] auf das echte Home-Verzeichnis auf. Ein
    /// Test, der §6.2.2 belegen will, kann kein echtes `~/.ssh` anlegen,
    /// ohne die Maschine des Nutzers anzufassen.
    home_override: Option<PathBuf>,
}

impl Default for OsKeyFileReader {
    fn default() -> Self {
        Self::new()
    }
}

impl OsKeyFileReader {
    pub fn new() -> Self {
        Self {
            home_override: None,
        }
    }

    /// Nur für Tests: §6.2.2 muss `~/…` belegen können, ohne im echten
    /// Home-Verzeichnis des Nutzers eine Datei anzulegen.
    #[cfg(test)]
    fn with_home(home: PathBuf) -> Self {
        Self {
            home_override: Some(home),
        }
    }

    fn home_dir(&self) -> Option<PathBuf> {
        match &self.home_override {
            Some(home) => Some(home.clone()),
            None => directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()),
        }
    }

    /// A-3: `~` am Anfang wird aufgelöst; ein relativer Pfad wird
    /// abgelehnt, weil „relativ wozu" beim Verbindungsaufbau keine
    /// beantwortbare Frage ist. **`~user` wird nicht unterstützt** — es
    /// bräuchte einen passwd-Lookup, den die Standardbibliothek nicht
    /// hergibt und den es auf Windows nicht gibt; ein solcher Pfad wird
    /// wie ein relativer behandelt (§6.2.8).
    ///
    /// Aufgelöst wird **hier**, nicht beim Speichern (§4.4): Der Nutzer hat
    /// `~` getippt und meint `~`; der aufgelöste Pfad hinge am Rechner.
    fn resolve_path(&self, path: &str) -> Result<PathBuf, KeyFileError> {
        let not_absolute = || KeyFileError::PathNotAbsolute {
            path: path.to_string(),
        };

        let expanded = if path == "~" {
            self.home_dir().ok_or_else(not_absolute)?
        } else if let Some(rest) = path.strip_prefix("~/") {
            let mut home = self.home_dir().ok_or_else(not_absolute)?;
            home.push(rest);
            home
        } else if path.starts_with('~') {
            // `~user/...` — A-3: ausdrücklich nicht unterstützt, und zwar
            // ohne jeden Versuch, den Nutzer aufzulösen.
            return Err(not_absolute());
        } else {
            PathBuf::from(path)
        };

        if !expanded.is_absolute() {
            return Err(not_absolute());
        }
        Ok(expanded)
    }

    /// Öffnet, prüft und liest — alles auf **einem** Handle.
    ///
    /// `enforce_permissions == true` bricht nach der Rechteprüfung ab,
    /// **bevor** gelesen wird: Eine Datei, die wir ohnehin ablehnen, wird
    /// gar nicht erst in den Speicher geholt.
    fn probe(&self, path: &str, enforce_permissions: bool) -> Result<Probe, KeyFileError> {
        let resolved = self.resolve_path(path)?;
        let file = open_readable(&resolved).map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => KeyFileError::NotFound {
                path: path.to_string(),
            },
            // Jeder andere Öffnungsfehler — fehlende Rechte, ein Pfad mit
            // NUL-Byte (`InvalidInput`), ein kaputter Link — ist aus Sicht
            // des Nutzers dasselbe: die Datei ist nicht lesbar. 5.4: Wir
            // nennen den Grund, durchsuchen aber nichts und schlagen
            // nichts vor.
            _ => KeyFileError::NotReadable {
                path: path.to_string(),
            },
        })?;

        // `fstat` auf dem Handle, nicht `stat` auf dem Pfad (A-3, §6.4.7).
        let meta = file.metadata().map_err(|_| KeyFileError::NotReadable {
            path: path.to_string(),
        })?;

        // A-3: nur reguläre Dateien. Ein Verzeichnis, ein Zeichengerät
        // (`/dev/zero`, `/dev/stdin`), ein Rohr oder ein Socket melden
        // zudem Größe 0 und kämen durch jede Größenprüfung.
        if !meta.is_file() {
            return Err(KeyFileError::NotARegularFile {
                path: path.to_string(),
            });
        }

        let (permissions_too_open, mode) = permission_facts(&meta);
        if enforce_permissions && permissions_too_open {
            return Err(KeyFileError::PermissionsTooOpen {
                path: path.to_string(),
                mode,
            });
        }

        // Erste der beiden Größenprüfungen (A-3): die schnelle Abkürzung am
        // `fstat`. Die verbindliche ist die Lesegrenze unten — es wird nie
        // mehr als 1 MiB in den Speicher gelesen, auch wenn `fstat`
        // weniger meldet.
        if meta.len() > MAX_KEY_FILE_BYTES {
            return Err(KeyFileError::TooLarge {
                path: path.to_string(),
                limit: MAX_KEY_FILE_BYTES,
            });
        }

        Ok(Probe {
            permissions_too_open,
            content: read_and_classify(file, path),
        })
    }
}

/// Zwischenbefund: der Rechte-Befund ist vom Lese-Ergebnis **getrennt**,
/// damit [`OsKeyFileReader::inspect`] beides melden kann (B-3) — auch dann,
/// wenn der Schlüssel selbst ungültig ist.
struct Probe {
    permissions_too_open: bool,
    content: Result<KeyFileContent, KeyFileError>,
}

/// A-3: unter Unix mit `O_NONBLOCK`, sonst hinge das Öffnen eines benannten
/// Rohrs, bis ein Schreiber erscheint. Auf Windows entfällt der Zusatz;
/// dort genügt die Prüfung auf dem Handle.
#[cfg(unix)]
fn open_readable(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_readable(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

/// A-4/E-1: „genauso ablehnen wie `ssh`". OpenSSH prüft
/// `(st_mode & 077) != 0` — also **jedes** Recht für Gruppe oder Welt, nicht
/// nur das Leserecht. Diese Fassung übernimmt die Regel wörtlich; sie ist
/// damit eher strenger als die Formulierung „für Gruppe oder Welt lesbar"
/// und in keinem Fall laxer.
///
/// Auf Windows entfällt die Prüfung, wie bei OpenSSH selbst.
#[cfg(unix)]
fn permission_facts(meta: &std::fs::Metadata) -> (bool, u32) {
    use std::os::unix::fs::MetadataExt;
    let mode = meta.mode() & 0o777;
    (mode & 0o077 != 0, mode)
}

#[cfg(not(unix))]
fn permission_facts(_meta: &std::fs::Metadata) -> (bool, u32) {
    (false, 0)
}

/// Liest höchstens [`MAX_KEY_FILE_BYTES`] + 1 Byte und prüft den Inhalt auf
/// einen gültigen OpenSSH-Schlüssel.
///
/// Das eine Byte über der Grenze ist der Unterschied zwischen „genau 1 MiB"
/// (erlaubt) und „mehr als 1 MiB" (abgelehnt), ohne dafür mehr lesen zu
/// müssen. Ein Objekt, das seine Größe mit 0 meldet und trotzdem endlos
/// liefert, läuft hier auf — die `fstat`-Prüfung oben ist nur die
/// Abkürzung, diese Grenze ist die verbindliche (A-3).
///
/// 5.1: Schlüsselmaterial landet in einer [`SecretString`]. Die
/// Zwischenform ist unvermeidbar — eine `SecretString` entsteht über einen
/// `String`, also über eine UTF-8-Prüfung —, wird aber kurz gehalten und
/// auf jedem Fehlerpfad überschrieben.
fn read_and_classify(mut file: File, path: &str) -> Result<KeyFileContent, KeyFileError> {
    use secrecy::zeroize::Zeroize;

    let invalid = || KeyFileError::InvalidKey {
        path: path.to_string(),
    };

    let mut buf = Vec::new();
    let read_result = file
        .by_ref()
        .take(MAX_KEY_FILE_BYTES + 1)
        .read_to_end(&mut buf);

    if read_result.is_err() {
        buf.zeroize();
        return Err(KeyFileError::NotReadable {
            path: path.to_string(),
        });
    }
    if buf.len() as u64 > MAX_KEY_FILE_BYTES {
        buf.zeroize();
        return Err(KeyFileError::TooLarge {
            path: path.to_string(),
            limit: MAX_KEY_FILE_BYTES,
        });
    }

    // A-6: „nicht als Text lesbar (kein UTF-8, NUL-Bytes)" ergibt dieselbe
    // Meldung wie ein ungültiger Schlüssel. NUL wird ausdrücklich geprüft,
    // weil U+0000 gültiges UTF-8 ist und die Prüfung darunter also nicht
    // greifen würde.
    if buf.contains(&0) {
        buf.zeroize();
        return Err(invalid());
    }

    let classification = classify_openssh_key(&buf);
    let encrypted = match classification {
        KeyClassification::Valid { encrypted } => encrypted,
        KeyClassification::Invalid => {
            buf.zeroize();
            return Err(invalid());
        }
    };

    let text = match String::from_utf8(buf) {
        Ok(text) => text,
        Err(err) => {
            let mut bytes = err.into_bytes();
            bytes.zeroize();
            return Err(invalid());
        }
    };

    Ok(KeyFileContent {
        key: SecretString::from(text),
        encrypted,
    })
}

impl KeyFileReader for OsKeyFileReader {
    fn read(&self, path: &str, enforce_permissions: bool) -> Result<KeyFileContent, KeyFileError> {
        self.probe(path, enforce_permissions)?.content
    }

    fn inspect(&self, path: &str) -> KeyFileFacts {
        // B-3/C-7: derselbe Weg wie `read`, nur ohne den Schlüssel
        // herauszugeben — und ohne Rechte-Sperre, damit der Befund den
        // Mangel **melden** kann, statt an ihm zu scheitern (A-4, letzter
        // Absatz).
        match self.probe(path, false) {
            Ok(probe) => match probe.content {
                Ok(content) => KeyFileFacts {
                    exists: true,
                    permissions_too_open: probe.permissions_too_open,
                    valid_key: true,
                    encrypted: content.encrypted,
                    problem: None,
                },
                Err(problem) => KeyFileFacts {
                    exists: true,
                    permissions_too_open: probe.permissions_too_open,
                    valid_key: false,
                    encrypted: false,
                    problem: Some(problem),
                },
            },
            Err(problem) => KeyFileFacts {
                // Ein Pfad, der gar keiner ist, und eine fehlende Datei
                // sind die beiden Fälle, in denen nichts existiert; bei
                // allen übrigen gibt es etwas, es taugt nur nicht.
                exists: !matches!(
                    problem,
                    KeyFileError::NotFound { .. } | KeyFileError::PathNotAbsolute { .. }
                ),
                permissions_too_open: matches!(problem, KeyFileError::PermissionsTooOpen { .. }),
                valid_key: false,
                encrypted: false,
                problem: Some(problem),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use ssh_transport::test_keys;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn reader() -> OsKeyFileReader {
        OsKeyFileReader::new()
    }

    /// Legt eine Datei an und setzt unter Unix die Rechte. Auf Windows
    /// bleibt `mode` wirkungslos — wie A-4 es vorsieht.
    fn write_file(dir: &Path, name: &str, content: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).expect("Testdatei schreiben");
        set_mode(&path, mode);
        path
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("Rechte setzen");
    }

    #[cfg(not(unix))]
    fn set_mode(_path: &Path, _mode: u32) {}

    fn as_str(path: &Path) -> String {
        path.to_str().expect("Testpfade sind gültiges UTF-8").into()
    }

    // --- §6.2.1/§6.2.9: der gute Fall ---------------------------------

    /// §6.2.1: Absoluter Pfad, Rechte `0600`, gültiger Schlüssel →
    /// gelesen, byte-gleich zum Dateiinhalt.
    #[test]
    fn test_absolute_path_with_valid_key_is_read() {
        let dir = TempDir::new().unwrap();
        let key = test_keys::unencrypted_private_key();
        let path = write_file(dir.path(), "id_ed25519", &key, 0o600);

        let content = reader()
            .read(&as_str(&path), true)
            .expect("eine korrekt geschützte, gültige Schlüsseldatei muss lesbar sein");

        assert_eq!(content.key.expose_secret(), key);
        assert!(!content.encrypted);
    }

    /// A-5: Ein verschlüsselter Schlüssel ist eine **Feststellung**, kein
    /// Fehler. Der Test scheitert, sobald das Lesen den Fall selbst als
    /// Fehler behandelt — dann wäre jeder verschlüsselte Schlüssel
    /// unbenutzbar, auch mit korrekt hinterlegter Passphrase.
    #[test]
    fn test_encrypted_key_is_reported_not_rejected() {
        let dir = TempDir::new().unwrap();
        let key = test_keys::encrypted_private_key("correct horse");
        let path = write_file(dir.path(), "id_enc", &key, 0o600);

        let content = reader()
            .read(&as_str(&path), true)
            .expect("A-5: das Lesen entscheidet nicht über die Passphrase");

        assert!(content.encrypted, "der Schlüssel ist verschlüsselt");
        assert_eq!(content.key.expose_secret(), key, "C-4: byte-gleich");
    }

    /// §6.2.9: `inspect` meldet denselben Befund wie `read` — und gibt
    /// **nie** Schlüsselmaterial heraus (das ist am Typ [`KeyFileFacts`]
    /// abzulesen, der kein Feld dafür hat).
    #[test]
    fn test_inspect_matches_read_for_a_valid_encrypted_key() {
        let dir = TempDir::new().unwrap();
        let key = test_keys::encrypted_private_key("pw");
        let path = write_file(dir.path(), "id_enc", &key, 0o600);

        let facts = reader().inspect(&as_str(&path));

        assert_eq!(
            facts,
            KeyFileFacts {
                exists: true,
                permissions_too_open: false,
                valid_key: true,
                encrypted: true,
                problem: None,
            }
        );
    }

    // --- §6.2.2/§6.2.3/§6.2.8: Pfadformen ------------------------------

    /// §6.2.2: `~/…` wird aufgelöst; derselbe Schlüssel wird gefunden.
    #[test]
    fn test_tilde_is_expanded_to_the_home_directory() {
        let home = TempDir::new().unwrap();
        let ssh_dir = home.path().join(".ssh");
        fs::create_dir(&ssh_dir).unwrap();
        let key = test_keys::unencrypted_private_key();
        write_file(&ssh_dir, "id_ed25519", &key, 0o600);

        let content = OsKeyFileReader::with_home(home.path().to_path_buf())
            .read("~/.ssh/id_ed25519", true)
            .expect("~ muss beim Lesen aufgelöst werden (A-3)");

        assert_eq!(content.key.expose_secret(), key);
    }

    /// §6.2.3: Ein relativer Pfad wird abgelehnt — „relativ wozu" ist beim
    /// Verbinden keine beantwortbare Frage (A-3).
    #[test]
    fn test_relative_path_is_rejected() {
        let err = reader()
            .read(".ssh/id_ed25519", true)
            .err()
            .expect("ein relativer Pfad ist kein gültiger Ort für einen Schlüssel");

        assert!(
            matches!(err, KeyFileError::PathNotAbsolute { .. }),
            "erwartet PathNotAbsolute, bekam {err:?}"
        );
    }

    /// §6.2.8: `~user/…` ergibt denselben Fehler — **nicht** etwa einen
    /// Versuch, den Nutzer aufzulösen (A-3: kein passwd-Lookup).
    #[test]
    fn test_tilde_user_is_rejected_without_trying_to_resolve_the_user() {
        let err = reader()
            .read("~root/.ssh/id_ed25519", true)
            .err()
            .expect("~user wird nicht unterstützt");

        assert!(
            matches!(err, KeyFileError::PathNotAbsolute { .. }),
            "erwartet PathNotAbsolute, bekam {err:?}"
        );
    }

    // --- §6.2.4/§6.2.10: Rechte ---------------------------------------

    /// §6.2.4: Rechte `0644` → abgelehnt, mit `chmod`-Befehl in der
    /// Meldung (A-4/E-1). Auf Windows entfällt die Prüfung, wie bei
    /// OpenSSH selbst.
    #[test]
    #[cfg(unix)]
    fn test_world_readable_file_is_rejected_for_login_with_a_chmod_hint() {
        let dir = TempDir::new().unwrap();
        let key = test_keys::unencrypted_private_key();
        let path = write_file(dir.path(), "id_ed25519", &key, 0o644);
        let path_str = as_str(&path);

        let err = reader()
            .read(&path_str, true)
            .err()
            .expect("A-4: eine offen liegende Schlüsseldatei sperrt die Anmeldung");

        assert!(
            matches!(err, KeyFileError::PermissionsTooOpen { .. }),
            "erwartet PermissionsTooOpen, bekam {err:?}"
        );
        let message = err.to_string();
        assert!(
            message.contains(&format!("chmod 600 {path_str}")),
            "die Meldung muss sagen, wie es zu beheben ist: {message}"
        );
    }

    /// §6.2.10 und der Grund für den Schalter überhaupt (§4.2): Dieselbe
    /// `0644`-Datei ist mit `enforce_permissions: false` **lesbar** — sonst
    /// wäre der Rettungsknopf aus C-1 genau dann gesperrt, wenn er
    /// gebraucht wird.
    #[test]
    #[cfg(unix)]
    fn test_world_readable_file_is_readable_when_permissions_are_not_enforced() {
        let dir = TempDir::new().unwrap();
        let key = test_keys::unencrypted_private_key();
        let path = write_file(dir.path(), "id_ed25519", &key, 0o644);

        let content = reader()
            .read(&as_str(&path), false)
            .expect("C-3: die Überführung muss genau so eine Datei lesen können");

        assert_eq!(content.key.expose_secret(), key);
    }

    /// A-4, letzter Absatz: Der **Befund** meldet den Mangel, statt ihn
    /// zur Sperre zu machen (B-3, C-7).
    #[test]
    #[cfg(unix)]
    fn test_inspect_reports_open_permissions_instead_of_failing() {
        let dir = TempDir::new().unwrap();
        let key = test_keys::unencrypted_private_key();
        let path = write_file(dir.path(), "id_ed25519", &key, 0o644);

        let facts = reader().inspect(&as_str(&path));

        assert!(facts.exists);
        assert!(
            facts.permissions_too_open,
            "der Mangel muss gemeldet werden"
        );
        assert!(facts.valid_key, "und der Schlüssel bleibt trotzdem gültig");
        assert_eq!(facts.problem, None, "der Befund selbst scheitert nicht");
    }

    /// E-1: „genauso ablehnen wie `ssh`" — OpenSSH prüft `(st_mode & 077)
    /// != 0`, also **jedes** Recht für Gruppe oder Welt, nicht nur das
    /// Leserecht. Der Test scheitert, sobald jemand die Prüfung auf `044`
    /// verengt („nur lesbar").
    #[test]
    #[cfg(unix)]
    fn test_group_write_only_is_also_rejected_like_openssh_does() {
        let dir = TempDir::new().unwrap();
        let key = test_keys::unencrypted_private_key();
        // 0620: für die Gruppe **schreib**-, nicht lesbar.
        let path = write_file(dir.path(), "id_ed25519", &key, 0o620);

        let err = reader()
            .read(&as_str(&path), true)
            .err()
            .expect("auch ein Gruppen-Schreibrecht ist zu weit");

        assert!(
            matches!(err, KeyFileError::PermissionsTooOpen { .. }),
            "erwartet PermissionsTooOpen, bekam {err:?}"
        );
    }

    // --- §6.2.5: symbolische Links ------------------------------------

    /// §6.2.5/E-2: Einem Link wird gefolgt — und die Rechteprüfung greift
    /// auf der Datei, die **tatsächlich gelesen wird**, nicht auf dem Link
    /// (der unter Unix immer `0777` hat und sonst jede Anmeldung sperren
    /// würde).
    #[test]
    #[cfg(unix)]
    fn test_symlink_to_a_valid_key_is_followed_and_target_permissions_apply() {
        let dir = TempDir::new().unwrap();
        let key = test_keys::unencrypted_private_key();
        let target = write_file(dir.path(), "real_key", &key, 0o600);
        let link = dir.path().join("link_to_key");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let content = reader()
            .read(&as_str(&link), true)
            .expect("E-2: einem Link wird gefolgt, der Link selbst ist nicht der Maßstab");

        assert_eq!(content.key.expose_secret(), key);
    }

    // --- §6.2.6: Größe -------------------------------------------------

    /// §6.2.6: Eine Datei über 1 MiB wird abgelehnt — und die Umsetzung
    /// liest sie **nicht** vollständig in den Speicher (die Lesegrenze ist
    /// `MAX_KEY_FILE_BYTES + 1`, nicht die Dateigröße).
    #[test]
    fn test_file_larger_than_the_limit_is_rejected() {
        let dir = TempDir::new().unwrap();
        let oversized = "A".repeat((MAX_KEY_FILE_BYTES + 1024) as usize);
        let path = write_file(dir.path(), "huge", &oversized, 0o600);

        let err = reader()
            .read(&as_str(&path), true)
            .err()
            .expect("über 1 MiB wird nicht gelesen");

        assert!(
            matches!(err, KeyFileError::TooLarge { .. }),
            "erwartet TooLarge, bekam {err:?}"
        );
        assert!(
            err.to_string().contains(&MAX_KEY_FILE_BYTES.to_string()),
            "die Meldung muss die Grenze nennen: {err}"
        );
    }

    /// Gegenprobe zur Grenze: Genau 1 MiB ist **erlaubt** — die Grenze ist
    /// „größer als", nicht „ab". Ohne diesen Test wäre eine
    /// Off-by-one-Verschärfung unbemerkt.
    #[test]
    fn test_a_file_of_exactly_the_limit_is_not_rejected_for_size() {
        let dir = TempDir::new().unwrap();
        let exactly = "A".repeat(MAX_KEY_FILE_BYTES as usize);
        let path = write_file(dir.path(), "exact", &exactly, 0o600);

        let err = reader()
            .read(&as_str(&path), true)
            .err()
            .expect("es ist kein Schlüssel — aber eben auch nicht zu groß");

        assert!(
            matches!(err, KeyFileError::InvalidKey { .. }),
            "genau 1 MiB darf nicht an der Größe scheitern, bekam {err:?}"
        );
    }

    // --- §6.2.7/§6.4.6: keine regulären Dateien ------------------------

    /// §6.2.7: Ein Verzeichnis ist keine reguläre Datei.
    #[test]
    fn test_directory_is_rejected() {
        let dir = TempDir::new().unwrap();

        let err = reader()
            .read(&as_str(dir.path()), true)
            .err()
            .expect("ein Verzeichnis ist kein Schlüssel");

        assert!(
            matches!(err, KeyFileError::NotARegularFile { .. }),
            "erwartet NotARegularFile, bekam {err:?}"
        );
    }

    /// §6.2.7/§6.4.6: Ein Zeichengerät meldet Größe 0 und käme durch jede
    /// Größenprüfung — `/dev/zero` würde beim Lesen endlos liefern.
    #[test]
    #[cfg(unix)]
    fn test_character_device_is_rejected() {
        for device in ["/dev/zero", "/dev/stdin"] {
            let err = reader()
                .read(device, false)
                .err()
                .expect("{device} ist keine reguläre Datei");
            assert!(
                matches!(
                    err,
                    KeyFileError::NotARegularFile { .. } | KeyFileError::NotReadable { .. }
                ),
                "{device}: erwartet NotARegularFile/NotReadable, bekam {err:?}"
            );
        }
    }

    /// §6.2.7 — **der eigentliche Prüfpunkt**: Ein benanntes Rohr ohne
    /// Schreiber darf **nicht blockieren**. Genau das erzwingt `O_NONBLOCK`
    /// beim Öffnen (A-3); ohne das Flag hinge dieser Test, bis ihn jemand
    /// abbricht, statt sauber rot zu werden.
    ///
    /// Der Test belegt damit zugleich, dass die selbst gehaltene
    /// `O_NONBLOCK`-Konstante auf dieser Plattform den richtigen Wert hat
    /// (s. deren Doc-Kommentar).
    #[test]
    #[cfg(unix)]
    fn test_named_pipe_is_rejected_without_blocking() {
        let dir = TempDir::new().unwrap();
        let fifo = dir.path().join("pipe");
        let Some(()) = make_fifo(&fifo) else {
            // Kein `mkfifo` auf diesem System — dann gibt es auch nichts
            // zu prüfen. Sichtbar übersprungen, nicht still grün.
            eprintln!("mkfifo nicht verfügbar — §6.2.7 (Rohr) übersprungen");
            return;
        };
        let path = as_str(&fifo);

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // `KeyFileContent` hat bewusst kein `Debug` (5.1) — über den
            // Kanal geht deshalb nur der **Fehler**, nie der Inhalt.
            let outcome = match OsKeyFileReader::new().read(&path, false) {
                Ok(_) => "unerwartet gelesen".to_string(),
                Err(err) => format!("{err:?}"),
            };
            let _ = tx.send(outcome);
        });

        let outcome = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("A-3: das Öffnen eines Rohrs ohne Schreiber darf nicht hängen");
        assert!(
            outcome.contains("NotARegularFile"),
            "ein Rohr ist keine reguläre Datei, bekam {outcome}"
        );
    }

    /// Legt ein benanntes Rohr an. Ohne `libc` im Arbeitsbereich geht das
    /// nur über `mkfifo(1)` — in einem Test völlig ausreichend und die
    /// ehrlichere Variante, als dafür eine Abhängigkeit aufzunehmen.
    #[cfg(unix)]
    fn make_fifo(path: &Path) -> Option<()> {
        let status = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .ok()?;
        status.success().then_some(())
    }

    /// §6.4.6: Ein Link auf ein Verzeichnis landet nach dem Folgen beim
    /// Verzeichnis — und damit bei „keine reguläre Datei".
    #[test]
    #[cfg(unix)]
    fn test_symlink_to_a_directory_is_rejected() {
        let dir = TempDir::new().unwrap();
        let link = dir.path().join("link_to_dir");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();

        let err = reader()
            .read(&as_str(&link), false)
            .err()
            .expect("auch über einen Link bleibt ein Verzeichnis ein Verzeichnis");

        assert!(
            matches!(err, KeyFileError::NotARegularFile { .. }),
            "erwartet NotARegularFile, bekam {err:?}"
        );
    }

    /// §6.4.6: Ein Pfad mit NUL-Byte ergibt einen sauberen Fehler — kein
    /// Panic, kein Hänger.
    #[test]
    fn test_path_containing_a_nul_byte_fails_cleanly() {
        let err = reader()
            .read("/tmp/key\0extra", true)
            .err()
            .expect("ein Pfad mit NUL-Byte lässt sich nicht öffnen");

        assert!(
            matches!(
                err,
                KeyFileError::NotReadable { .. } | KeyFileError::NotFound { .. }
            ),
            "erwartet einen sauberen Lesefehler, bekam {err:?}"
        );
    }

    /// §6.4.6: Ein absoluter Pfad mit `..` bleibt absolut und wird ganz
    /// normal gelesen — wir normalisieren nichts (§4.4) und verbieten
    /// nichts, was der Nutzer mit seinen eigenen Rechten ohnehin lesen
    /// darf (5.4).
    #[test]
    fn test_absolute_path_with_dotdot_resolves_through_the_filesystem() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        let key = test_keys::unencrypted_private_key();
        write_file(dir.path(), "id_ed25519", &key, 0o600);
        let path = as_str(&sub.join("..").join("id_ed25519"));

        let content = reader()
            .read(&path, true)
            .expect("das Dateisystem löst `..` auf, wir nicht");

        assert_eq!(content.key.expose_secret(), key);
    }

    // --- §6.4.2: keine Fehlermeldung verrät Inhalt ---------------------

    /// §6.4.2: Fünf Dateien — leer, Textdatei, Binärdatei mit NUL-Bytes,
    /// ein **öffentlicher** statt eines privaten Schlüssels, ein
    /// abgeschnittener privater Schlüssel. Jede ergibt „kein gültiger
    /// Schlüssel" mit Pfad, und in **keiner** Meldung steht ein Byte aus
    /// der Datei (5.2).
    ///
    /// Der Test scheitert, sobald jemand den Fehlertext der
    /// Parse-Bibliothek durchreicht oder „nur kurz" den Dateianfang
    /// mitliefert.
    #[test]
    fn test_no_error_message_ever_reveals_file_content() {
        let dir = TempDir::new().unwrap();
        let valid = test_keys::unencrypted_private_key();
        let truncated: String = valid.lines().take(2).collect::<Vec<_>>().join("\n");

        // Jeder Fall trägt eine erkennbare Zeichenfolge, nach der die
        // Meldung durchsucht wird. Bei der leeren Datei gibt es nichts zu
        // verraten — sie bleibt trotzdem drin, weil sie ein eigener
        // Fehlerpfad ist.
        let cases: Vec<(&str, String, Option<&str>)> = vec![
            ("empty", String::new(), None),
            (
                "plain_text",
                "TOTALLY-SECRET-MARKER-42\n".to_string(),
                Some("TOTALLY-SECRET-MARKER-42"),
            ),
            (
                "binary_with_nul",
                String::from_utf8(vec![b'M', b'A', b'R', b'K', 0, 0, b'E', b'R']).unwrap(),
                Some("MARK"),
            ),
            ("public_key", test_keys::public_key(), None),
            ("truncated_private_key", truncated, None),
        ];

        for (name, content, marker) in cases {
            let path = write_file(dir.path(), name, &content, 0o600);
            let path_str = as_str(&path);

            let err = reader()
                .read(&path_str, true)
                .err()
                .unwrap_or_else(|| panic!("{name}: darf nicht als Schlüssel durchgehen"));

            assert!(
                matches!(err, KeyFileError::InvalidKey { .. }),
                "{name}: A-6 verlangt „kein gültiger Schlüssel“, bekam {err:?}"
            );

            let message = err.to_string();
            assert!(
                message.contains(&path_str),
                "{name}: die Meldung muss den Pfad nennen — {message}"
            );
            if let Some(marker) = marker {
                assert!(
                    !message.contains(marker),
                    "{name}: 5.2 — kein Byte aus der Datei in der Meldung: {message}"
                );
            }
            // Auch der Debug-Ausdruck darf nichts verraten: Er landet in
            // Logzeilen genauso wie der Anzeigetext.
            let debug = format!("{err:?}");
            if let Some(marker) = marker {
                assert!(
                    !debug.contains(marker),
                    "{name}: 5.1/5.2 — kein Dateiinhalt im Debug-Ausdruck: {debug}"
                );
            }
        }
    }

    /// §6.4.3, auf Ebene der Leseumsetzung: Was nicht gelesen werden darf,
    /// ergibt **einen Fehler** — nie einen leeren oder ersatzweisen
    /// Schlüssel, mit dem die Anmeldung dann doch weiterliefe (5.3).
    #[test]
    fn test_a_failed_check_never_yields_a_usable_key() {
        let dir = TempDir::new().unwrap();
        let missing = as_str(&dir.path().join("does_not_exist"));

        for (case, path, enforce) in [
            ("fehlende Datei", missing.as_str(), true),
            ("relativer Pfad", "relative/id_ed25519", true),
            ("Verzeichnis", dir.path().to_str().unwrap(), true),
        ] {
            let result = reader().read(path, enforce);
            assert!(
                result.is_err(),
                "{case}: 5.3 verlangt Abbruch, kein Ersatzschlüssel"
            );
        }
    }

    // --- §6.4.7: Wettlauf beim Lesen -----------------------------------

    /// §6.4.7: Die Datei wird zwischen Rechteprüfung und Lesen gegen eine
    /// andere ausgetauscht. Erwartung: Prüfung und Lesen greifen auf
    /// **derselben** Datei — also auf demselben offenen Handle, nicht auf
    /// zwei Pfadzugriffen.
    ///
    /// Aufbau: Unter demselben Pfad wechseln sich zwei Dateien ab — eine
    /// mit `0600`, eine mit `0644`. Eine Umsetzung, die erst den Pfad
    /// `stat`et und dann öffnet, kann die `0600`-Rechte sehen und den
    /// Inhalt der `0644`-Datei zurückgeben. Genau das prüft die Zusicherung
    /// unten: Ein **erfolgreiches** `read` darf niemals den Inhalt der
    /// offen liegenden Datei liefern.
    #[test]
    #[cfg(unix)]
    fn test_permission_check_and_read_see_the_same_file() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc as StdArc;

        let dir = TempDir::new().unwrap();
        let protected_key = test_keys::unencrypted_private_key();
        let exposed_key = test_keys::unencrypted_private_key();
        assert_ne!(protected_key, exposed_key, "zwei verschiedene Schlüssel");

        let protected = write_file(dir.path(), "protected", &protected_key, 0o600);
        let exposed = write_file(dir.path(), "exposed", &exposed_key, 0o644);
        let target = dir.path().join("current");
        fs::rename(&protected, &target).unwrap();
        let target_str = as_str(&target);

        let stop = StdArc::new(AtomicBool::new(false));
        let swapper = {
            let stop = StdArc::clone(&stop);
            let dir = dir.path().to_path_buf();
            let target = target.clone();
            let (protected_key, exposed_key) = (protected_key.clone(), exposed_key.clone());
            std::thread::spawn(move || {
                let spare = dir.join("spare");
                let mut exposed_turn = true;
                while !stop.load(Ordering::Relaxed) {
                    let (content, mode) = if exposed_turn {
                        (&exposed_key, 0o644)
                    } else {
                        (&protected_key, 0o600)
                    };
                    fs::write(&spare, content).unwrap();
                    set_mode(&spare, mode);
                    // `rename` ist atomar: unter `target` steht immer eine
                    // vollständige Datei, nie eine halbe.
                    fs::rename(&spare, &target).unwrap();
                    exposed_turn = !exposed_turn;
                }
            })
        };

        let reader = reader();
        for _ in 0..2_000 {
            match reader.read(&target_str, true) {
                Ok(content) => assert_ne!(
                    content.key.expose_secret(),
                    exposed_key.as_str(),
                    "§6.4.7: die Rechteprüfung hat eine andere Datei gesehen als das Lesen — \
                     der Inhalt der offen liegenden Datei ist durchgekommen"
                ),
                Err(err) => assert!(
                    matches!(err, KeyFileError::PermissionsTooOpen { .. }),
                    "erwartet wird nur der Rechte-Fehler, bekam {err:?}"
                ),
            }
        }

        stop.store(true, Ordering::Relaxed);
        swapper.join().unwrap();
        let _ = exposed;
    }
}
