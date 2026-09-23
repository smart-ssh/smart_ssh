use std::fmt;

use secrecy::SecretString;

use crate::profiles::{AuthMethod, CredentialStore};

use super::error::SshError;

/// Obergrenze für eine Schlüsseldatei (Spec 0076, A-3): 1 MiB. Sie wird an
/// **zwei** Stellen geprüft — am `fstat` des offenen Handles (die schnelle
/// Abkürzung) und beim Lesen selbst (die verbindliche Grenze). Ein Objekt,
/// das seine Größe mit 0 meldet und trotzdem endlos liefert, kommt durch
/// die erste, nicht durch die zweite.
pub const MAX_KEY_FILE_BYTES: u64 = 1024 * 1024;

/// Was eine Schlüsseldatei hergibt, ohne Urteil darüber, ob es reicht
/// (Spec 0076, §4.2).
///
/// `encrypted` ist eine **Feststellung**, kein Fehler: Ob ein
/// verschlüsselter Schlüssel benutzbar ist, hängt an `passphrase_ref` und
/// entscheidet sich deshalb in [`resolve_auth`] (A-5), nicht beim Lesen.
/// Läge die Entscheidung im Lesen, könnte ein verschlüsselter Schlüssel
/// **mit** hinterlegter Passphrase nie verbinden.
///
/// Kein abgeleitetes `Debug`: `key` ist Schlüsselmaterial und bleibt in
/// [`SecretString`] (5.1).
pub struct KeyFileContent {
    pub key: SecretString,
    pub encrypted: bool,
}

/// Der Vorab-Befund über eine Schlüsseldatei (Spec 0076, §4.2) — für den
/// Befund vor dem Speichern (B-3) und für die Frage, ob die Überführung
/// wählbar ist (C-7). Gibt **nie** Schlüsselmaterial heraus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFileFacts {
    pub exists: bool,
    /// Nur Unix; auf Windows immer `false`, wie bei OpenSSH selbst (A-4,
    /// E-1).
    pub permissions_too_open: bool,
    pub valid_key: bool,
    pub encrypted: bool,
    /// Der Grund, wenn die Datei gar nicht erst in Frage kommt — für die
    /// Meldung aus C-7 und B-3.
    pub problem: Option<KeyFileError>,
}

/// Eine Variante je pfadtragender Zeile aus Spec 0076, A-6 — bewusst
/// **kein** Zeichenketten-Fehler: Die Tests aus §6.1.5 prüfen gegen die
/// Variante, nicht gegen den Meldungstext. Ein Test gegen Text wäre kaum
/// zum Scheitern zu bringen und würde beim nächsten Umformulieren rot.
///
/// `PassphraseNoetig` ist bewusst **keine** Variante: Diese Entscheidung
/// fällt in [`resolve_auth`], das `passphrase_ref` kennt (A-5).
///
/// **Keine Variante trägt Dateiinhalt** (5.2). `PermissionsTooOpen` trägt
/// den Dateimodus — Metadaten, kein Inhalt — damit die Meldung sagen kann,
/// was tatsächlich zu weit steht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyFileError {
    NotFound { path: String },
    NotReadable { path: String },
    PermissionsTooOpen { path: String, mode: u32 },
    TooLarge { path: String, limit: u64 },
    NotARegularFile { path: String },
    PathNotAbsolute { path: String },
    InvalidKey { path: String },
}

impl KeyFileError {
    /// Der Pfad, den dieser Fehler betrifft — jede Variante trägt ihn
    /// (§4.2), weil nur wer ihn kennt, ihn auch nennen kann.
    pub fn path(&self) -> &str {
        match self {
            KeyFileError::NotFound { path }
            | KeyFileError::NotReadable { path }
            | KeyFileError::PermissionsTooOpen { path, .. }
            | KeyFileError::TooLarge { path, .. }
            | KeyFileError::NotARegularFile { path }
            | KeyFileError::PathNotAbsolute { path }
            | KeyFileError::InvalidKey { path } => path,
        }
    }

    /// Stabiler, sprachunabhängiger Bezeichner je Fehlerart — dasselbe
    /// Muster wie [`SshError::code`] (Spec 0024, Abschnitt 5).
    pub fn code(&self) -> &'static str {
        match self {
            KeyFileError::NotFound { .. } => "KEY_FILE_NOT_FOUND",
            KeyFileError::NotReadable { .. } => "KEY_FILE_NOT_READABLE",
            KeyFileError::PermissionsTooOpen { .. } => "KEY_FILE_PERMISSIONS_TOO_OPEN",
            KeyFileError::TooLarge { .. } => "KEY_FILE_TOO_LARGE",
            KeyFileError::NotARegularFile { .. } => "KEY_FILE_NOT_A_REGULAR_FILE",
            KeyFileError::PathNotAbsolute { .. } => "KEY_FILE_PATH_NOT_ABSOLUTE",
            KeyFileError::InvalidKey { .. } => "KEY_FILE_INVALID_KEY",
        }
    }
}

impl fmt::Display for KeyFileError {
    /// Spec 0076, A-6: jede Zeile nennt **Pfad und Grund**, nie eine Zeile
    /// aus der Datei (5.2). Insbesondere bei `InvalidKey` wird der
    /// Fehlertext der Parse-Bibliothek **nicht** durchgereicht — ob deren
    /// `Display` je Eingabebytes wiedergibt, wissen wir nicht, und eine
    /// Sicherheits-Invariante darf nicht am Anzeigeverhalten einer fremden
    /// Kiste hängen.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyFileError::NotFound { path } => {
                write!(f, "Die Schlüsseldatei {path} wurde nicht gefunden")
            }
            KeyFileError::NotReadable { path } => {
                write!(f, "Die Schlüsseldatei {path} ist nicht lesbar")
            }
            KeyFileError::PermissionsTooOpen { path, mode } => write!(
                f,
                "Die Schlüsseldatei {path} ist für Gruppe oder Welt lesbar \
                 (Rechte {mode:04o}). Zu beheben mit: chmod 600 {path}",
            ),
            KeyFileError::TooLarge { path, limit } => write!(
                f,
                "Die Schlüsseldatei {path} ist größer als {limit} Byte (1 MiB) \
                 und wird nicht gelesen",
            ),
            KeyFileError::NotARegularFile { path } => {
                write!(f, "{path} ist keine reguläre Datei")
            }
            KeyFileError::PathNotAbsolute { path } => write!(
                f,
                "Für die Schlüsseldatei ist ein absoluter Pfad nötig; {path} ist keiner",
            ),
            KeyFileError::InvalidKey { path } => {
                write!(f, "Die Datei {path} enthält keinen gültigen Schlüssel")
            }
        }
    }
}

impl std::error::Error for KeyFileError {}

/// Die äußere Grenze zum Dateisystem (Spec 0076, §1.3/§4.2) — `core` liest
/// keine Datei selbst, so wie es auch keine SSH-Verbindung selbst aufbaut.
/// Die konkrete Umsetzung liegt in `app-shell`, in Tests steht eine
/// Attrappe ([`super::mock::MockKeyFileReader`]).
///
/// Kein `Send + Sync`-Bound am Trait selbst — dasselbe Vorgehen wie bei
/// [`CredentialStore`]: der Bound wird erst am Objekttyp der Aufrufstelle
/// ergänzt (`&(dyn KeyFileReader + Send + Sync)`), damit der Trait
/// synchron und app-unabhängig bleibt.
pub trait KeyFileReader {
    /// Öffnet die Datei und prüft Pfadform, Dateiart, Rechte und Größe auf
    /// **demselben** Handle (A-3, §6.4.7), liest den Schlüssel und prüft
    /// ihn auf Gültigkeit. Alle pfadtragenden Fehler aus A-6 entstehen
    /// hier; `core` prüft nichts davon nach.
    ///
    /// `enforce_permissions`: `true` für die Anmeldung (A-4 lehnt eine zu
    /// weit geöffnete Datei ab), `false` für die Überführung (C-3) und den
    /// Import — dort wird der Mangel gemeldet, nicht zur Sperre. Ohne
    /// diesen Schalter wäre der Rettungsknopf aus C-1 genau dann gesperrt,
    /// wenn er gebraucht wird.
    fn read(&self, path: &str, enforce_permissions: bool) -> Result<KeyFileContent, KeyFileError>;

    /// Derselbe Weg, ohne den Schlüssel herauszugeben (B-3, C-7).
    fn inspect(&self, path: &str) -> KeyFileFacts;
}

/// Aufgelöstes Auth-Material für einen [`super::Hop`].
///
/// Kein `russh`-spezifischer Typ — die Übersetzung in konkrete `russh`-
/// Auth-Aufrufe ist Sache von `crates/ssh-transport` (Spec 0005 Abschnitt
/// 2). Nicht Teil der in Abschnitt 4-7 der Spec explizit vorgegebenen
/// Typen, aber eine direkte, naheliegende Konsequenz aus Abschnitt 8
/// ("Fehler-Mapping ... fehlende Credentials führen zu
/// `CredentialResolutionFailed`") und Teil 2 Punkt 3 der Aufgabenstellung:
/// Die *Auflösung* von `AuthMethod` + `CredentialStore` zu tatsächlichem
/// Auth-Material ist reine Logik ohne Netz-I/O (nur `CredentialStore`-
/// Lookups, selbst synchron) — gehört also nach `core`, nicht in die
/// `russh`-spezifische Crate, die nur noch das bereits aufgelöste Material
/// gegen `russh`s Auth-API verwenden soll.
#[derive(Debug, Clone)]
pub enum ResolvedAuth {
    Password(SecretString),
    PrivateKey {
        key: SecretString,
        passphrase: Option<SecretString>,
    },
    Agent,
    Certificate {
        cert: SecretString,
        key: SecretString,
    },
}

/// Löst ein [`AuthMethod`] über einen [`CredentialStore`] zu tatsächlichem
/// Auth-Material auf. Fehlende/ungültige Credentials ergeben
/// [`SshError::CredentialResolutionFailed`] mit einer verständlichen
/// Meldung, nie einen Panic (Spec 0005 Abschnitt 8; Teil 2, Punkt 3 der
/// Aufgabenstellung).
///
/// Spec 0076, §4.2: `key_files` ist die äußere Grenze zum Dateisystem, die
/// [`AuthMethod::IdentityFile`] braucht. Sie reist überall neben
/// `credentials` mit, weil sie dieselbe Herkunft und dieselbe Lebensdauer
/// hat.
pub fn resolve_auth(
    auth: &AuthMethod,
    credentials: &dyn CredentialStore,
    key_files: &dyn KeyFileReader,
) -> Result<ResolvedAuth, SshError> {
    match auth {
        AuthMethod::Password { credential_ref } => {
            let secret = credentials
                .get(credential_ref)
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Passwort: {e}")))?;
            Ok(ResolvedAuth::Password(secret))
        }
        AuthMethod::PrivateKey {
            credential_ref,
            passphrase_ref,
        } => {
            let key = credentials
                .get(credential_ref)
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Private Key: {e}")))?;
            let passphrase = passphrase_ref
                .as_ref()
                .map(|r| credentials.get(r))
                .transpose()
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Passphrase: {e}")))?;
            Ok(ResolvedAuth::PrivateKey { key, passphrase })
        }
        AuthMethod::Agent => Ok(ResolvedAuth::Agent),
        AuthMethod::Certificate { cert_ref, key_ref } => {
            let cert = credentials
                .get(cert_ref)
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Zertifikat: {e}")))?;
            let key = credentials
                .get(key_ref)
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Key: {e}")))?;
            Ok(ResolvedAuth::Certificate { cert, key })
        }
        // Spec 0076, A-2: bildet auf **dieselbe** Variante ab wie ein
        // gespeicherter Schlüssel. `ssh-transport` und `russh` sehen keinen
        // Unterschied; der Unterschied ist die Herkunft, nicht das
        // Material. Eine eigene `ResolvedAuth::IdentityFile`-Variante wäre
        // eine zweite Fassung derselben Sache (§4.1).
        AuthMethod::IdentityFile {
            path,
            passphrase_ref,
        } => {
            // A-4: `enforce_permissions: true` — auf dem **Anmelde**-Pfad
            // lehnt eine zu weit geöffnete Datei ab. Die Überführung (C-3)
            // ruft dieselbe Methode mit `false`.
            let content = key_files
                .read(path, true)
                .map_err(|e| SshError::CredentialResolutionFailed(e.to_string()))?;
            let passphrase = passphrase_ref
                .as_ref()
                .map(|r| credentials.get(r))
                .transpose()
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Passphrase: {e}")))?;

            // A-5: **Hier** fällt die Entscheidung, nicht beim Lesen. Die
            // Datei weiß nichts von `passphrase_ref`; läge die Prüfung im
            // Lesen, könnte ein verschlüsselter Schlüssel mit korrekt
            // hinterlegter Passphrase nie verbinden. Und nur diese Stelle
            // kennt den Pfad und kann ihn in der Meldung nennen (A-6).
            if content.encrypted && passphrase.is_none() {
                return Err(SshError::CredentialResolutionFailed(format!(
                    "Der Schlüssel in {path} ist verschlüsselt, aber es ist keine \
                     Passphrase hinterlegt"
                )));
            }

            Ok(ResolvedAuth::PrivateKey {
                key: content.key,
                passphrase,
            })
        }
    }
}
