//! Version- & Build-Anzeige (Spec 0052): der kurze Git-Commit-Hash wird von
//! `build.rs` als `SMART_SSH_BUILD_HASH`-Umgebungsvariable zur Compile-Zeit
//! gesetzt (`"unknown"`, falls kein Git verfügbar war — s. dortiger
//! Doc-Kommentar) und hier über `env!` in eine Konstante übernommen. Die
//! Version selbst kommt weiterhin aus `tauri::AppHandle::package_info()`
//! (liest `tauri.conf.json`, Spec 0048) — nur der Hash ist neu.

/// Kurzer Git-Commit-Hash des gebauten Stands, z. B. `"a5b3e01"`, oder
/// `"unknown"` ohne Git zur Build-Zeit (s. `build.rs`).
pub const BUILD_COMMIT_HASH: &str = env!("SMART_SSH_BUILD_HASH");

/// Das eine, überall geteilte Anzeigeformat aus Spec 0052, Abschnitt 1:
/// `"0.4.1 (a5b3e01)"`. Genutzt von der Startup-Logzeile, dem
/// Diagnosepaket und [`crate::commands::get_app_info`] (dessen
/// `AppInfoDto::version_display`-Feld). Titelzeile (`AppHeader.tsx`) und
/// Über-Dialog (`AboutSettings.tsx`) übernehmen diesen Teil unverändert und
/// hängen nur Edition und Build-Typ ([`BuildType`]) mit ` · ` an — den
/// Versions-/Hash-Teil selbst setzt das Frontend nie neu zusammen.
pub fn version_with_hash(version: &str) -> String {
    format!("{version} ({BUILD_COMMIT_HASH})")
}

/// Ob Debug- (`cargo tauri dev`) oder Release-Build. Dev-Builds nutzen ein
/// eigenes Datenverzeichnis ("Smart SSH (dev)", s. `persistence_sqlite::
/// paths`, außer bei gesetztem `SMART_SSH_DATA_DIR`) — deshalb in
/// Über-Dialog, Titelzeile, Startup-Log und Diagnosepaket sichtbar.
/// Serialisiert als `"Dev"`/`"Release"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum BuildType {
    Dev,
    Release,
}

impl BuildType {
    pub const fn current() -> Self {
        if cfg!(debug_assertions) {
            Self::Dev
        } else {
            Self::Release
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "Dev",
            Self::Release => "Release",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_with_hash_matches_spec_format() {
        // `BUILD_COMMIT_HASH` ist zur Testlaufzeit dieses Checkouts
        // gesetzt (Test läuft aus einem echten Git-Repo) — der Test prüft
        // bewusst nur das *Format* ("<version> (<hash>)"), nicht den
        // konkreten Hash-Wert, der sich mit jedem Commit ändert.
        let result = version_with_hash("0.4.1");

        assert!(result.starts_with("0.4.1 ("));
        assert!(result.ends_with(')'));
        assert_eq!(result, format!("0.4.1 ({BUILD_COMMIT_HASH})"));
    }
}
