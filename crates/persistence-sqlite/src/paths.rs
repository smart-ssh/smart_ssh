use std::path::PathBuf;

use directories::ProjectDirs;

/// Plattformspezifischer Standard-Pfad für die App-Datenbankdatei
/// (Spec 0004, Abschnitt 3):
///
/// - macOS: `~/Library/Application Support/ssh-manager/ssh-manager.db`
/// - Windows: `%APPDATA%\ssh-manager\ssh-manager.db`
/// - Linux: `~/.local/share/ssh-manager/ssh-manager.db`
///
/// Öffentlich (nicht nur intern von [`crate::SqliteProfileStore::connect`]
/// genutzt), damit `app-tauri` später denselben Pfad ermitteln kann, ohne
/// die Pfad-Logik zu duplizieren (Aufgabenstellung Teil 2, Punkt 5).
///
/// Qualifier und Organisation werden bewusst leer gelassen
/// (`ProjectDirs::from("", "", "ssh-manager")`) — die in der Spec
/// vorgegebenen Beispielpfade enthalten keinen zusätzlichen
/// Organisations-/Reverse-DNS-Anteil, nur den schlichten Ordnernamen
/// "ssh-manager".
///
/// Panic-Signatur bewusst wie in der Spec/Aufgabenstellung vorgegeben
/// (`-> PathBuf`, kein `Option`/`Result`): `ProjectDirs::from` liefert nur
/// dann `None`, wenn das Betriebssystem kein Home-Verzeichnis für den
/// aktuellen Nutzer ermitteln kann — für eine Desktop-App ein praktisch
/// nicht behebbarer Umgebungsfehler, kein regulärer, vom Aufrufer
/// behandelbarer Fall.
pub fn default_db_path() -> PathBuf {
    let dirs = ProjectDirs::from("", "", "ssh-manager")
        .expect("kein Home-Verzeichnis gefunden – kann App-Datenordner nicht ermitteln");
    dirs.data_dir().join("ssh-manager.db")
}
