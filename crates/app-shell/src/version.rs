//! Version- & Build-Anzeige (Spec 0052): der kurze Git-Commit-Hash wird von
//! `build.rs` als `SMART_SSH_BUILD_HASH`-Umgebungsvariable zur Compile-Zeit
//! gesetzt (`"unknown"`, falls kein Git verfügbar war — s. dortiger
//! Doc-Kommentar) und hier über `env!` in eine Konstante übernommen. Die
//! Version selbst kommt weiterhin aus `tauri::AppHandle::package_info()`
//! (liest `tauri.conf.json`, Spec 0048) — nur der Hash ist neu.

/// Kurzer Git-Commit-Hash des gebauten Stands, z. B. `"a5b3e01"`, oder
/// `"unknown"` ohne Git zur Build-Zeit (s. `build.rs`).
///
/// `#[allow(dead_code)]`: dieser Commit legt nur die Grundlage (Spec 0052,
/// Abschnitt 7, Schritt 1); die Verwendungsstelle (Startzeilen-Log) folgt
/// im nächsten Commit — bis dahin wäre die Konstante ungenutzt und
/// `-D warnings` würde den Gate-Lauf sonst grundlos rot färben.
#[allow(dead_code)]
pub const BUILD_COMMIT_HASH: &str = env!("SMART_SSH_BUILD_HASH");

/// Das eine, überall geteilte Anzeigeformat aus Spec 0052, Abschnitt 1:
/// `"0.4.1 (a5b3e01)"`. Genutzt von der Startup-Logzeile, dem
/// Über-Dialog (über [`crate::commands::get_app_info`]) und der
/// Titelzeile (Frontend baut denselben String selbst aus den einzelnen
/// Feldern von `AppInfoDto` nach, s. `AppHeader.tsx`) — eine Stelle statt
/// drei auseinanderlaufender `format!`-Aufrufe.
///
/// `#[allow(dead_code)]`: s. Kommentar auf [`BUILD_COMMIT_HASH`] —
/// entfernt im nächsten Commit, sobald die Startzeile sie nutzt.
#[allow(dead_code)]
pub fn version_with_hash(version: &str) -> String {
    format!("{version} ({BUILD_COMMIT_HASH})")
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
