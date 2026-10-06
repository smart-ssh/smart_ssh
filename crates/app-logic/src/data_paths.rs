//! Issue #16: Wo Smart SSH seine Daten ablegt — die wirksamen Pfade der
//! laufenden Instanz für die Anzeige in den Einstellungen (Abschnitt
//! „Diagnose").
//!
//! **Keine zweite Quelle der Wahrheit:** Jeder Pfad kommt aus derselben
//! Funktion, die auch der Code nutzt, der die Datei tatsächlich liest oder
//! schreibt:
//!
//! - Datenbank: [`persistence_sqlite::default_db_path`] (inkl.
//!   `SMART_SSH_DATA_DIR` und Debug-Suffix, ADR 0032)
//! - Logs: [`crate::logging::default_log_dir`]
//! - Host-Keys: [`crate::startup_error_messages::host_key_store_path`]
//! - Verpackungsdatei des Master-Passworts:
//!   [`crate::master_password::wrapping_file_path`]
//! - MCP-Einstellungen: kommt vom Aufrufer (`app-shell`), weil der Ort an
//!   `tauri-plugin-store` hängt und nur mit einem `AppHandle` auflösbar ist
//!   (s. ADR 0105).
//!
//! Nur Pfade, nie Dateiinhalte — der Befehl darf keine Geheimnisse
//! preisgeben.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Stabile Bezeichner der eingebauten Einträge. Das Frontend übersetzt sie
/// in Beschriftungen; [`open_folder_target`] sucht über sie den Ordner.
pub const DATABASE: &str = "database";
pub const LOGS: &str = "logs";
pub const HOST_KEYS: &str = "hostKeys";
pub const WRAPPING_FILE: &str = "wrappingFile";
pub const MCP_SETTINGS: &str = "mcpSettings";

/// Ein zusätzlicher Pfad, den eine Edition über `Wiring` beisteuert (z. B.
/// eine Lizenzdatei). Die Community-Edition liefert keinen.
///
/// `label` ist Anzeigetext der Edition — der Übersetzungskatalog des
/// Frontends kennt editionsspezifische Einträge nicht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraDataPath {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
    pub is_directory: bool,
}

/// Ein angezeigter Pfad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPathEntry {
    pub id: String,
    /// `None` für eingebaute Einträge (Beschriftung aus dem
    /// Übersetzungskatalog), `Some` für Einträge einer Edition.
    pub label: Option<String>,
    pub path: PathBuf,
    pub is_directory: bool,
}

impl DataPathEntry {
    /// Der Ordner, den „Ordner öffnen" zeigt: der Pfad selbst bei einem
    /// Verzeichnis, sonst das Verzeichnis, in dem die Datei liegt.
    pub fn folder(&self) -> Option<&Path> {
        if self.is_directory {
            Some(&self.path)
        } else {
            self.path.parent()
        }
    }
}

/// DTO für das Frontend (`get_data_paths`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataPathEntryDto {
    pub id: String,
    pub label: Option<String>,
    pub path: String,
    pub is_directory: bool,
}

impl From<&DataPathEntry> for DataPathEntryDto {
    fn from(entry: &DataPathEntry) -> Self {
        Self {
            id: entry.id.clone(),
            label: entry.label.clone(),
            path: entry.path.display().to_string(),
            is_directory: entry.is_directory,
        }
    }
}

/// Baut die Liste aus bereits ermittelten Basispfaden. Rein, ohne Zugriff
/// auf Umgebung oder Dateisystem — testbar ohne Prozesszustand.
pub fn data_paths_for(
    db_path: &Path,
    log_dir: &Path,
    mcp_settings_path: &Path,
    extra: &[ExtraDataPath],
) -> Vec<DataPathEntry> {
    let builtin = |id: &str, path: PathBuf, is_directory: bool| DataPathEntry {
        id: id.to_string(),
        label: None,
        path,
        is_directory,
    };
    let mut entries = vec![
        builtin(DATABASE, db_path.to_path_buf(), false),
        builtin(LOGS, log_dir.to_path_buf(), true),
        builtin(
            HOST_KEYS,
            crate::startup_error_messages::host_key_store_path(db_path),
            false,
        ),
        builtin(
            WRAPPING_FILE,
            crate::master_password::wrapping_file_path(db_path),
            false,
        ),
        builtin(MCP_SETTINGS, mcp_settings_path.to_path_buf(), false),
    ];
    entries.extend(extra.iter().map(|e| DataPathEntry {
        id: e.id.clone(),
        label: Some(e.label.clone()),
        path: e.path.clone(),
        is_directory: e.is_directory,
    }));
    entries
}

/// Die wirksamen Pfade der laufenden Instanz.
pub fn effective_data_paths(
    mcp_settings_path: &Path,
    extra: &[ExtraDataPath],
) -> Vec<DataPathEntry> {
    data_paths_for(
        &persistence_sqlite::default_db_path(),
        &crate::logging::default_log_dir(),
        mcp_settings_path,
        extra,
    )
}

/// Der Ordner zum Eintrag `id`, den „Ordner öffnen" zeigen soll.
///
/// Das Frontend schickt nur den Bezeichner, nie einen Pfad: Sonst könnte
/// jeder Code im Webview einen beliebigen Pfad im Dateimanager öffnen
/// lassen (dasselbe Muster wie `read_credential_file`, Spec 0013).
pub fn open_folder_target(entries: &[DataPathEntry], id: &str) -> Result<PathBuf, String> {
    let entry = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("unknown data path: {id}"))?;
    entry
        .folder()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("no folder for data path: {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(entries: &[DataPathEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.id.as_str()).collect()
    }

    #[test]
    fn test_builtin_entries_derive_from_the_existing_path_functions() {
        let db = PathBuf::from("/data/smart-ssh.db");
        let entries = data_paths_for(
            &db,
            Path::new("/logs"),
            Path::new("/cfg/settings.json"),
            &[],
        );

        assert_eq!(
            ids(&entries),
            vec![DATABASE, LOGS, HOST_KEYS, WRAPPING_FILE, MCP_SETTINGS]
        );
        assert_eq!(entries[0].path, db);
        assert_eq!(entries[1].path, PathBuf::from("/logs"));
        assert!(entries[1].is_directory);
        assert_eq!(
            entries[2].path,
            crate::startup_error_messages::host_key_store_path(&db)
        );
        assert_eq!(entries[2].path, PathBuf::from("/data/host_keys.json"));
        assert_eq!(
            entries[3].path,
            crate::master_password::wrapping_file_path(&db)
        );
        assert_eq!(
            entries[3].path,
            PathBuf::from("/data/smart-ssh.db.master-key")
        );
        assert_eq!(entries[4].path, PathBuf::from("/cfg/settings.json"));
        assert!(entries.iter().all(|e| e.label.is_none()));
    }

    #[test]
    fn test_community_edition_has_no_extra_entries_and_editions_can_add_some() {
        let base = data_paths_for(
            Path::new("/d/smart-ssh.db"),
            Path::new("/l"),
            Path::new("/c/settings.json"),
            &[],
        );
        assert_eq!(base.len(), 5);

        let extra = [ExtraDataPath {
            id: "license".into(),
            label: "License file".into(),
            path: PathBuf::from("/d/license.json"),
            is_directory: false,
        }];
        let with_extra = data_paths_for(
            Path::new("/d/smart-ssh.db"),
            Path::new("/l"),
            Path::new("/c/settings.json"),
            &extra,
        );
        assert_eq!(with_extra.len(), 6);
        let last = with_extra.last().unwrap();
        assert_eq!(last.id, "license");
        assert_eq!(last.label.as_deref(), Some("License file"));
        assert_eq!(last.path, PathBuf::from("/d/license.json"));
    }

    #[test]
    fn test_open_folder_target_uses_parent_for_files_and_the_dir_itself_for_directories() {
        let entries = data_paths_for(
            Path::new("/d/smart-ssh.db"),
            Path::new("/l/logs"),
            Path::new("/c/settings.json"),
            &[],
        );
        assert_eq!(
            open_folder_target(&entries, DATABASE).unwrap(),
            PathBuf::from("/d")
        );
        assert_eq!(
            open_folder_target(&entries, HOST_KEYS).unwrap(),
            PathBuf::from("/d")
        );
        assert_eq!(
            open_folder_target(&entries, LOGS).unwrap(),
            PathBuf::from("/l/logs")
        );
        assert_eq!(
            open_folder_target(&entries, MCP_SETTINGS).unwrap(),
            PathBuf::from("/c")
        );
    }

    /// Das Frontend schickt nur Bezeichner. Ein Pfad statt eines
    /// Bezeichners wird abgelehnt, nicht geöffnet.
    #[test]
    fn test_open_folder_target_rejects_unknown_ids_including_raw_paths() {
        let entries = data_paths_for(
            Path::new("/d/smart-ssh.db"),
            Path::new("/l"),
            Path::new("/c/settings.json"),
            &[],
        );
        assert!(open_folder_target(&entries, "/etc").is_err());
        assert!(open_folder_target(&entries, "nope").is_err());
    }

    #[test]
    fn test_dto_serializes_camel_case_and_path_only() {
        let entry = DataPathEntry {
            id: WRAPPING_FILE.into(),
            label: None,
            path: PathBuf::from("/d/smart-ssh.db.master-key"),
            is_directory: false,
        };
        let json = serde_json::to_value(DataPathEntryDto::from(&entry)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "wrappingFile",
                "label": null,
                "path": "/d/smart-ssh.db.master-key",
                "isDirectory": false,
            })
        );
    }

    /// Serialisiert die Tests, die `SMART_SSH_DATA_DIR` setzen —
    /// Umgebungsvariablen sind Prozess-global.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Räumt die Variable auch nach einem fehlschlagenden Assert auf.
    struct EnvVarGuard;
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: durch `ENV_LOCK` serialisiert; kein anderer Test in
            // diesem Crate liest oder schreibt `SMART_SSH_DATA_DIR`.
            unsafe {
                std::env::remove_var("SMART_SSH_DATA_DIR");
            }
        }
    }

    /// Issue #16, AC 2: Mit `SMART_SSH_DATA_DIR` zeigen Datenbank,
    /// Host-Keys und Verpackungsdatei in das Override-Verzeichnis.
    #[test]
    fn test_effective_paths_follow_smart_ssh_data_dir_override() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _cleanup = EnvVarGuard;
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: s. `EnvVarGuard`.
        unsafe {
            std::env::set_var("SMART_SSH_DATA_DIR", dir.path());
        }

        let entries = effective_data_paths(Path::new("/c/settings.json"), &[]);
        let path_of = |id: &str| {
            entries
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.path.clone())
                .unwrap()
        };

        assert_eq!(path_of(DATABASE), dir.path().join("smart-ssh.db"));
        assert_eq!(path_of(HOST_KEYS), dir.path().join("host_keys.json"));
        assert_eq!(
            path_of(WRAPPING_FILE),
            dir.path().join("smart-ssh.db.master-key")
        );
        assert_eq!(
            open_folder_target(&entries, DATABASE).unwrap(),
            dir.path().to_path_buf()
        );
        // Logs hängen nicht am Override (Spec 0016): derselbe Ort wie ohne.
        assert_eq!(path_of(LOGS), crate::logging::default_log_dir());
    }
}
