//! DTOs: elevation.

use serde::Serialize;

/// Spec 0067, A3: Ergebnis von `sftp_elevation_enable`. Ein Fehlschlag ist
/// kein `Err`, sondern `failure` — das Frontend zeigt dazu eine
/// verständliche Erklärung samt kopierbarer sudoers-Zeile.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ElevationResultDto {
    pub active: bool,
    pub target_user: String,
    /// Ermittelter bzw. konfigurierter Pfad, sofern bekannt.
    pub sftp_server_path: Option<String>,
    pub failure: Option<ElevationFailureDto>,
    /// Hinweis bei aktivem Kanal (Spec 0067, A3): `sftp-server` bzw. ein
    /// übergeordnetes Verzeichnis ist nicht sicher geschützt.
    pub warning: Option<ElevationWarningDto>,
    /// Issue #622: Operationen, die noch über den erhöhten Kanal laufen.
    /// Nicht leer nur, wenn `sftp_elevation_enable` (Nutzerwechsel) ohne
    /// Bestätigung aufgerufen wurde: nichts wurde geändert, der bisherige
    /// Kanal bleibt aktiv (`active`, `target_user` beschreiben ihn).
    #[serde(default)]
    pub running_operations: Vec<RunningOperationDto>,
    /// Issue #1192: `sftp-server` runs with `-l INFO -f AUTHPRIV`, so each
    /// file operation shows up in the server's auth log.
    #[serde(default)]
    pub server_logging: bool,
    /// Issue #1195: how sudo authenticated for the active channel
    /// (`"nopasswd"` | `"password"`, never the password); `None` when not
    /// active.
    #[serde(default)]
    pub sudo_auth: Option<&'static str>,
}

/// Issue #622: eine laufende Operation des erhöhten Kanals — für den
/// Bestätigungsdialog vor dem Ausschalten/Umschalten.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct RunningOperationDto {
    pub id: String,
    /// `download` | `upload` | `delete` | `chmod`.
    pub kind: &'static str,
    pub path: String,
    pub bytes_done: Option<u64>,
    pub bytes_total: Option<u64>,
    pub files_done: Option<u64>,
    pub files_total: Option<u64>,
}

/// Issue #622: Ergebnis von `sftp_elevation_disable`. `disabled == false`
/// mit nicht leerer `running_operations` heißt: nichts geändert, der Nutzer
/// muss das Stoppen bestätigen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ElevationDisableDto {
    pub disabled: bool,
    pub running_operations: Vec<RunningOperationDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ElevationWarningDto {
    pub kind: ElevationWarningKind,
    /// Der auffällige Eintrag (Datei oder Verzeichnis).
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum ElevationWarningKind {
    SftpServerUnsafe,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ElevationFailureDto {
    pub kind: ElevationFailureKind,
    /// Zugeschnittene Zeile `<login> ALL=(<nutzer>) NOPASSWD: <pfad> -l INFO -f AUTHPRIV` —
    /// nur, wenn eine fehlende sudo-Regel die Ursache ist.
    pub sudoers_line: Option<String>,
    /// Technisches Detail (erste stderr-Zeile o. ä.), nur zur Anzeige.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum ElevationFailureKind {
    /// Lokaler Pseudo-Server o. ä. — kein erhöhter Modus möglich.
    Unsupported,
    InvalidUser,
    InvalidPath,
    SftpServerNotFound,
    /// `sftp-server` oder ein übergeordnetes Verzeichnis gehört nicht root
    /// bzw. ist für Gruppe/andere beschreibbar; `detail` ist der Pfad.
    SftpServerUnsafe,
    PasswordRequired,
    /// Issue #1191: the stored sudo password was rejected by sudo.
    WrongSudoPassword,
    NotAllowed,
    RequireTty,
    SudoMissing,
    CheckFailed,
    StartFailed,
    /// Issue #1194: the server profile does not allow elevated rights.
    DisabledForServer,
}
