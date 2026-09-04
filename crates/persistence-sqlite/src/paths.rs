use std::path::PathBuf;

use directories::BaseDirs;

/// Überschreibt sowohl den Standard-Datenpfad als auch den Debug-Build-
/// Suffix (s. [`default_db_path`]) — wirkt in Debug- UND Release-Builds
/// gleichermaßen (bewusst, damit sich auch ein Release-Artefakt gezielt
/// gegen ein sauberes/bestimmtes Verzeichnis testen lässt, nicht nur
/// `cargo tauri dev`). Leerer Wert wird wie "nicht gesetzt" behandelt,
/// nicht als "aktuelles Verzeichnis" — ein versehentlich leer
/// exportiertes `SMART_SSH_DATA_DIR=` soll nicht stillschweigend auf
/// einen kaum sinnvollen relativen Pfad zeigen.
const DATA_DIR_OVERRIDE_ENV: &str = "SMART_SSH_DATA_DIR";

/// Ermittelt das App-Datenverzeichnis — Grundlage für [`default_db_path`]
/// und (über dessen `.parent()`) den Host-Key-Speicher
/// (`app-tauri::lib::build_app_state`). Reihenfolge:
///
/// 1. `SMART_SSH_DATA_DIR`, falls gesetzt und nicht leer (s. o.).
/// 2. Sonst der plattformübliche Datenpfad — im Release-Build unverändert
///    wie zuvor ("Smart SSH"/`smart-ssh`, identisch für Community- und
///    Official-Edition, s. Architektur-Brief D2), im **Debug**-Build
///    (`cargo tauri dev`, `#[cfg(debug_assertions)]`) mit einem
///    zusätzlichen "(dev)"/`-dev`-Suffix.
///
/// Hintergrund: vor dieser Trennung teilten sich `cargo tauri dev` und
/// ein lokal gebautes Release-Binary dieselbe SQLite-DB — ein Dev-Build
/// mit einer neueren Migration hinterließ eine DB, an der ein
/// Release-Build von einem älteren Commit beim Start mit
/// `Migrate(VersionMissing(n))` scheiterte (unbekannte, "aus der Zukunft"
/// stammende Migrationsnummer). Der Debug-Suffix ist bewusst NUR ein
/// Debug-Build-Merkmal, keine Editions-Trennung — Community und Official
/// (beide Release-Builds, `debug_assertions` aus) bleiben unverändert
/// identisch, wie von D2 verlangt (s.
/// `docs/adr/0032-dev-data-dir-separation-vs-d2.md`).
fn resolve_data_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var(DATA_DIR_OVERRIDE_ENV) {
        if !override_dir.is_empty() {
            return PathBuf::from(override_dir);
        }
    }

    let base = BaseDirs::new()
        .expect("kein Home-Verzeichnis gefunden – kann App-Datenordner nicht ermitteln");

    #[cfg(all(target_os = "linux", debug_assertions))]
    let app_dir_name = "smart-ssh-dev";
    #[cfg(all(target_os = "linux", not(debug_assertions)))]
    let app_dir_name = "smart-ssh";
    #[cfg(all(not(target_os = "linux"), debug_assertions))]
    let app_dir_name = "Smart SSH (dev)";
    #[cfg(all(not(target_os = "linux"), not(debug_assertions)))]
    let app_dir_name = "Smart SSH";

    base.data_dir().join(app_dir_name)
}

/// Plattformspezifischer Standard-Pfad für die App-Datenbankdatei
/// (Spec 0004, Abschnitt 3, nach der Umbenennung zu "Smart SSH"; Debug-
/// Suffix und `SMART_SSH_DATA_DIR`-Override s. [`resolve_data_dir`], Spec
/// 0041):
///
/// - macOS (Release): `~/Library/Application Support/Smart SSH/smart-ssh.db`
/// - macOS (Debug/`cargo tauri dev`): `~/Library/Application Support/Smart SSH (dev)/smart-ssh.db`
/// - Windows (Release): `%APPDATA%\Smart SSH\smart-ssh.db`
/// - Windows (Debug): `%APPDATA%\Smart SSH (dev)\smart-ssh.db`
/// - Linux (Release): `~/.local/share/smart-ssh/smart-ssh.db`
/// - Linux (Debug): `~/.local/share/smart-ssh-dev/smart-ssh.db`
///
/// Öffentlich (nicht nur intern von [`crate::SqliteProfileStore::connect`]
/// genutzt), damit `app-tauri` später denselben Pfad ermitteln kann, ohne
/// die Pfad-Logik zu duplizieren (Aufgabenstellung Teil 2, Punkt 5).
///
/// **`BaseDirs` statt `ProjectDirs`**: `ProjectDirs::from("", "", "Smart SSH")`
/// würde den App-Namen für den Ordnernamen selbst normalisieren (empirisch
/// geprüft: macOS ergäbe `Application Support/Smart-SSH`, Leerzeichen durch
/// Bindestrich ersetzt) — das weicht von den in der Spec vorgegebenen
/// Pfaden ab, die auf macOS/Windows den Ordnernamen mit Leerzeichen
/// verlangen ("Smart SSH"), auf Linux dagegen klein/mit Bindestrich
/// ("smart-ssh"). `BaseDirs::data_dir()` liefert stattdessen nur das
/// unqualifizierte System-Datenverzeichnis (macOS:
/// `~/Library/Application Support`, Windows: `%APPDATA%`, Linux:
/// `~/.local/share`, ohne App-spezifischen Unterordner) — den
/// Ordner-/Dateinamen hängt diese Funktion selbst an, mit exakter Kontrolle
/// über Groß-/Kleinschreibung pro Plattform.
///
/// Panic-Signatur bewusst wie in der Spec/Aufgabenstellung vorgegeben
/// (`-> PathBuf`, kein `Option`/`Result`): `BaseDirs::new` liefert nur dann
/// `None`, wenn das Betriebssystem kein Home-Verzeichnis für den aktuellen
/// Nutzer ermitteln kann — für eine Desktop-App ein praktisch nicht
/// behebbarer Umgebungsfehler, kein regulärer, vom Aufrufer behandelbarer
/// Fall.
pub fn default_db_path() -> PathBuf {
    resolve_data_dir().join("smart-ssh.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialisiert alle Tests dieses Moduls — sie mutieren einen
    /// Prozess-globalen Zustand (Umgebungsvariablen), parallele
    /// `cargo test`-Threads würden sich sonst gegenseitig überschreiben.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Räumt `SMART_SSH_DATA_DIR` beim Drop wieder auf, selbst wenn eine
    /// Assertion im Testkörper fehlschlägt/panict — sonst könnte ein
    /// fehlschlagender Test die Variable für nachfolgende Tests gesetzt
    /// lassen (`cargo test` führt alle Tests eines Binaries im selben
    /// Prozess aus).
    struct EnvVarGuard;
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: durch `ENV_LOCK` serialisiert — kein anderer Thread
            // dieses Prozesses liest/schreibt `SMART_SSH_DATA_DIR`
            // gleichzeitig (s. `std::env::set_var`-Doku zur fehlenden
            // Thread-Sicherheit ohne externe Synchronisierung).
            unsafe {
                std::env::remove_var(DATA_DIR_OVERRIDE_ENV);
            }
        }
    }

    #[test]
    fn test_data_dir_override_env_wins_over_default_and_debug_suffix() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _cleanup = EnvVarGuard;
        // SAFETY: s. `EnvVarGuard`-Doc-Kommentar oben.
        unsafe {
            std::env::set_var(DATA_DIR_OVERRIDE_ENV, "/tmp/smart-ssh-test-override");
        }

        assert_eq!(
            resolve_data_dir(),
            PathBuf::from("/tmp/smart-ssh-test-override")
        );
        assert_eq!(
            default_db_path(),
            PathBuf::from("/tmp/smart-ssh-test-override/smart-ssh.db")
        );
    }

    #[test]
    fn test_empty_data_dir_override_env_is_treated_as_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _cleanup = EnvVarGuard;
        // SAFETY: s. `EnvVarGuard`-Doc-Kommentar oben.
        unsafe {
            std::env::set_var(DATA_DIR_OVERRIDE_ENV, "");
        }

        // Ein leerer Wert darf nicht als "aktuelles Verzeichnis" (`PathBuf::from("")`)
        // durchschlagen — muss auf den regulären Default zurückfallen.
        let resolved = resolve_data_dir();
        assert_ne!(resolved, PathBuf::from(""));
        assert!(
            resolved.to_string_lossy().contains("Smart SSH")
                || resolved.to_string_lossy().contains("smart-ssh"),
            "erwartete den regulären Default-Pfad, war: {resolved:?}"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_build_default_path_carries_dev_suffix() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Sicherstellen, dass kein Override aus einem vorherigen Test
        // (oder der Umgebung des Testläufers) hier hineinwirkt.
        // SAFETY: s. `EnvVarGuard`-Doc-Kommentar oben.
        unsafe {
            std::env::remove_var(DATA_DIR_OVERRIDE_ENV);
        }

        let path = default_db_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("(dev)") || path_str.contains("-dev"),
            "Debug-Build muss einen erkennbaren Dev-Suffix tragen, war: {path_str}"
        );
        assert!(
            !path_str.contains("smart-ssh-dev-dev"),
            "kein doppelter Suffix erwartet: {path_str}"
        );
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn test_release_build_default_path_has_no_dev_suffix() {
        let _guard = ENV_LOCK.lock().unwrap();
        // SAFETY: s. `EnvVarGuard`-Doc-Kommentar oben.
        unsafe {
            std::env::remove_var(DATA_DIR_OVERRIDE_ENV);
        }

        let path = default_db_path();
        let path_str = path.to_string_lossy();
        assert!(
            !path_str.contains("(dev)") && !path_str.contains("-dev"),
            "Release-Build darf keinen Dev-Suffix tragen — Community/Official müssen \
             identisch bleiben (D2): {path_str}"
        );
    }
}
