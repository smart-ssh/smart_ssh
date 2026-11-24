//! DTOs: meta.

use serde::Serialize;

/// Spec 0052, Abschnitt 1/3.2/3.3: Version + Commit-Hash + Edition für
/// Über-Dialog und Titelzeile — dasselbe DTO für beide, damit sich die
/// Anzeige nicht auseinanderentwickelt: `versionDisplay` ist bereits das
/// fertig formatierte `"0.4.1 (a5b3e01)"` (`crate::version::
/// version_with_hash`), die Titelzeile übernimmt es unverändert und hängt
/// nur noch ihren eigenen "— Early Access"-Zusatz an, statt den String
/// selbst aus `version`/`commitHash` neu zusammenzusetzen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct AppInfoDto {
    /// Aus `tauri.conf.json` (Spec 0048), z. B. `"0.4.1"`.
    pub version: String,
    /// Kurzer Git-Commit-Hash (`crate::version::BUILD_COMMIT_HASH`), z. B.
    /// `"a5b3e01"`, oder `"unknown"` ohne Git zur Build-Zeit.
    pub commit_hash: String,
    /// Das geteilte Anzeigeformat aus Spec 0052, Abschnitt 1:
    /// `"0.4.1 (a5b3e01)"` — vorformatiert, damit Log/Über-Dialog/
    /// Titelzeile nicht je einen eigenen `format!`-Aufruf brauchen.
    pub version_display: String,
    /// `"Community"`/`"Official"` (aus `Wiring::edition`, Spec 0038) — als
    /// String statt eines eigenen Frontend-Enums, da das Frontend damit
    /// nur anzeigt, nie verzweigt (Edition-spezifisches Verhalten wird
    /// serverseitig über `Entitlements`/`Wiring` entschieden, nie im
    /// Frontend, s. CLAUDE.md "No special-casing").
    pub edition: String,
    /// s. [`crate::version::BuildType`].
    pub build_type: crate::version::BuildType,
    /// Issue #1313: the full data directory, only with an active override
    /// (`--data-dir`, `SMART_SSH_DATA_DIR`, pointer file); `None` otherwise.
    pub data_dir: Option<String>,
    /// The same path shortened with `~` for the header marker.
    pub data_dir_display: Option<String>,
    /// Absolute path of `settings.json` inside the override directory; the
    /// webview opens the settings store through it. `None` without override.
    pub settings_store_path: Option<String>,
}
