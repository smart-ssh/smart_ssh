use std::path::PathBuf;

use directories::BaseDirs;

/// Plattformspezifischer Standard-Pfad für die App-Datenbankdatei
/// (Spec 0004, Abschnitt 3, nach der Umbenennung zu "Smart SSH"):
///
/// - macOS: `~/Library/Application Support/Smart SSH/smart-ssh.db`
/// - Windows: `%APPDATA%\Smart SSH\smart-ssh.db`
/// - Linux: `~/.local/share/smart-ssh/smart-ssh.db`
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
    let base = BaseDirs::new()
        .expect("kein Home-Verzeichnis gefunden – kann App-Datenordner nicht ermitteln");

    #[cfg(target_os = "linux")]
    let app_dir_name = "smart-ssh";
    #[cfg(not(target_os = "linux"))]
    let app_dir_name = "Smart SSH";

    base.data_dir().join(app_dir_name).join("smart-ssh.db")
}
