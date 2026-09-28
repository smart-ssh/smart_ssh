//! Strukturiertes JSON-Lines-Logging (Spec 0016, Abschnitt 2/3) — Aufbau des
//! globalen `tracing`-Subscribers beim App-Start sowie die
//! altersbasierte Aufbewahrung ("14 Tage") der Log-Dateien.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use directories::BaseDirs;
use tracing_appender::non_blocking::WorkerGuard;

/// Spec 0016, Abschnitt 3: "Aufbewahrung der letzten 14 Tage".
const MAX_LOG_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// Stefans Fund (2026-09): mit `tracing_appender::rolling::daily`s alter
/// Kurzform (`daily(dir, "smart-ssh.log")`) landete das Datum ans Ende des
/// Dateinamens (`smart-ssh.log.2026-09-19`) — das `.log` in der Mitte
/// bricht die dateityp-basierte Programm-/Icon-Zuordnung des
/// Betriebssystems, die auf eine ENDENDE Erweiterung angewiesen ist (jeder
/// Tageswechsel hätte sonst erneut zugeordnet werden müssen). Getrennt in
/// Präfix + Suffix (s. `init_logging`s `Builder`-Aufruf), damit
/// `tracing_appender` stattdessen `smart-ssh.2026-09-19.log` erzeugt —
/// `.log` bleibt am Ende, unabhängig vom Datum dazwischen.
const LOG_FILE_PREFIX: &str = "smart-ssh";
const LOG_FILE_SUFFIX: &str = "log";

/// Plattformspezifischer Log-Ordner (Spec 0016, Abschnitt 3):
///
/// - macOS: `~/Library/Logs/Smart SSH/`
/// - Windows: `%APPDATA%\Smart SSH\logs\`
/// - Linux: `~/.local/state/smart-ssh/logs/`
///
/// **Kein `directories::BaseDirs::data_dir()`-Wiederverwendungsmuster wie
/// bei `persistence_sqlite::default_db_path`**: `~/Library/Logs` ist auf
/// macOS ein eigenständiges Standardverzeichnis, das `BaseDirs` nicht als
/// eigenen Accessor anbietet (nur `data_dir` → `Application Support`) —
/// hier deshalb direkt über `home_dir()` zusammengesetzt. Windows nutzt
/// bewusst denselben Basisordner wie die DB (`data_dir` → `%APPDATA%`), nur
/// mit eigenem `logs`-Unterordner, weil das exakt dem in der Spec
/// vorgegebenen `%APPDATA%\Smart SSH\logs\` entspricht. Linux nutzt
/// `state_dir()` (XDG `$XDG_STATE_HOME`, Default `~/.local/state`) statt
/// `data_dir()` — Logs sind laut XDG-Basisverzeichnis-Spezifikation "state
/// data", nicht "data", und die Spec verlangt exakt diesen Pfad.
pub fn default_log_dir() -> PathBuf {
    let base =
        BaseDirs::new().expect("kein Home-Verzeichnis gefunden – kann Log-Ordner nicht ermitteln");

    #[cfg(target_os = "macos")]
    {
        base.home_dir()
            .join("Library")
            .join("Logs")
            .join("Smart SSH")
    }
    #[cfg(target_os = "windows")]
    {
        base.data_dir().join("Smart SSH").join("logs")
    }
    #[cfg(target_os = "linux")]
    {
        base.state_dir()
            .expect("kein XDG-State-Verzeichnis gefunden – kann Log-Ordner nicht ermitteln")
            .join("smart-ssh")
            .join("logs")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        base.data_dir().join("smart-ssh").join("logs")
    }
}

/// Löscht alle Dateien in `dir`, deren letzte Änderung (`mtime`) mehr als
/// `max_age` vor `now` liegt — Spec 0016, Abschnitt 3: "ältere Dateien
/// werden beim Start automatisch gelöscht". Bewusst rein dateibasiert
/// (Änderungszeit statt Parsen des `tracing-appender`-Dateinamensformats):
/// funktioniert unabhängig davon, wie genau `tracing_appender::rolling`
/// seine Dateien benennt, und räumt auch verwaiste Dateien aus einem
/// früheren Namensschema auf.
///
/// `now` als Parameter (statt intern `SystemTime::now()`) macht die
/// Funktion ohne echte 14-Tage-Wartezeit testbar — ein Test kann eine
/// frisch erstellte Datei durch ein weit in der Zukunft liegendes `now`
/// simulieren, ganz ohne die Datei-`mtime` selbst manipulieren zu müssen
/// (s. `tests::test_cleanup_old_logs_*`, die das trotzdem zusätzlich über
/// `File::set_modified` tun, um "neuere Dateien bleiben unangetastet"
/// direkt neben einer wirklich alten Datei zu prüfen).
pub fn cleanup_old_logs(dir: &Path, max_age: Duration, now: SystemTime) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        let age = now.duration_since(modified).unwrap_or_default();
        if age > max_age {
            // Best-effort: ein einzelner nicht löschbarer Log-Rest (z. B.
            // durch eine gleichzeitig laufende zweite Instanz gesperrt)
            // soll den App-Start nicht verhindern.
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// Spec 0063, Teil 3: die zuletzt beschriebene Log-Datei in `dir` (nach
/// `mtime` unter den Dateien, die mit [`LOG_FILE_PREFIX`] beginnen —
/// bewusst nicht das exakte `tracing_appender`-Datumssuffix geparst,
/// funktioniert also unabhängig vom genauen Namensschema). `None`, falls
/// der Ordner fehlt/leer ist oder keine passende, lesbare Datei enthält —
/// der Diagnose-Export degradiert dann auf "keine Log-Zeilen verfügbar"
/// statt abzustürzen.
///
/// Spec-reviewer-Fund (Follow-up-Review, Spec 0063): ursprünglich ohne
/// Präfix-Filter (wie [`cleanup_old_logs`], das dieselbe Annahme trifft) —
/// dort ist die Konsequenz eines Fremdartefakts mit neuerer `mtime` im
/// (app-exklusiven) Log-Ordner nur eine überzählige Löschung, hier aber
/// eine ungeprüfte Aufnahme in ein öffentlich geteiltes Diagnosepaket.
/// Deshalb hier zusätzlich eingegrenzt, dort unverändert gelassen (kein
/// Grund, dessen Verhalten zu ändern).
fn most_recently_modified_file(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(LOG_FILE_PREFIX) && name.ends_with(LOG_FILE_SUFFIX)
        })
        .max_by_key(|entry| {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        })
        .map(|entry| entry.path())
}

/// Spec 0063, Teil 3: die letzten (höchstens) `max_lines` Zeilen der
/// aktuellsten Log-Datei — Rohtext, **nicht** redigiert (das ist Sache des
/// Aufrufers, s. `crate::diagnostics`, bevor die Zeilen in ein geteiltes
/// Paket wandern). Best-effort wie [`cleanup_old_logs`]: kein Log-Ordner,
/// keine lesbare Datei oder ein I/O-Fehler liefert eine leere Liste statt
/// eines Fehlers — ein Diagnosepaket ohne Log-Auszug ist immer noch
/// nützlicher als ein abgebrochener Export.
pub fn read_last_log_lines(dir: &Path, max_lines: usize) -> Vec<String> {
    let Some(path) = most_recently_modified_file(dir) else {
        return Vec::new();
    };
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].iter().map(|s| s.to_string()).collect()
}

/// Richtet den globalen `tracing`-Subscriber ein: JSON-Lines in eine
/// täglich rotierende Datei im plattformspezifischen Log-Ordner (Spec
/// 0016, Abschnitt 2/3). Räumt vor dem Öffnen der aktuellen Datei alte
/// Log-Dateien auf (s. [`cleanup_old_logs`]).
///
/// Gibt den [`WorkerGuard`] des nicht-blockierenden Writers zurück — dieser
/// muss so lange am Leben bleiben, wie geloggt werden soll (er flusht den
/// internen Puffer beim `Drop`). Der Aufrufer (`crate::run`) hält ihn
/// deshalb als lokale Variable über die gesamte App-Laufzeit
/// (`tauri::Builder::run` blockiert bis zum Beenden der App, danach ist ein
/// finaler Flush ohnehin nicht mehr relevant).
///
/// Log-Level per `RUST_LOG`-Umgebungsvariable konfigurierbar
/// (`tracing_subscriber::EnvFilter`), Default `info` — reicht für Spec
/// 0016 Abschnitt 4 (Kontext/Chunks/Parsing/Filter-Entscheidung/SSH/
/// Lifecycle sind alle `info`, nicht `debug`), ohne die Log-Dateien mit
/// `trace`-Rauschen aus Bibliotheks-Crates aufzublähen.
pub fn init_logging() -> WorkerGuard {
    let dir = default_log_dir();
    if let Err(err) = fs::create_dir_all(&dir) {
        eprintln!(
            "Log-Ordner {} konnte nicht angelegt werden: {err}",
            dir.display()
        );
    }
    if let Err(err) = cleanup_old_logs(&dir, MAX_LOG_AGE, SystemTime::now()) {
        eprintln!("Alte Log-Dateien konnten nicht aufgeräumt werden: {err}");
    }

    // `Builder` statt der `rolling::daily(dir, prefix)`-Kurzform (s.
    // `LOG_FILE_PREFIX`-Doc-Kommentar): die Kurzform kennt nur einen
    // einzigen Namensteil und hängt das Datum dahinter an, wodurch `.log`
    // vor dem Datum landet (`smart-ssh.log.2026-09-19`) statt danach —
    // `filename_prefix`/`filename_suffix` getrennt ergibt stattdessen
    // `smart-ssh.2026-09-19.log`, mit der Datei-Erweiterung dort, wo
    // dateityp-basierte Betriebssystem-Zuordnung sie erwartet.
    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .build(&dir)
        .expect("Log-Datei-Rotation konnte nicht initialisiert werden");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Unabhängiger Review-Pass (Spec 0016): Log-Dateien tragen Kommando-
    // Text/System-Kontext (nur best-effort redigiert, s. `log_outgoing_
    // context`/`log_command_execution`) und verdienen dieselbe
    // Zugriffsbeschränkung wie die SQLite-DB und `host_keys.json` (0700/
    // 0600) statt der OS-Standardrechte (typ. 0755/0644, weltlesbar).
    harden_log_permissions(&dir);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .json()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(filter)
        .init();

    guard
}

/// Setzt den Log-Ordner auf `0700` und jede darin liegende Datei auf
/// `0600` — dasselbe Muster wie
/// `persistence_sqlite::store::SqliteProfileStore::connect` (DB-Datei) und
/// `host_key_store::write_atomically` (`host_keys.json`). Best-effort (wie
/// dort): ein fehlgeschlagenes `chmod` verhindert nicht den App-Start.
#[cfg(unix)]
fn harden_log_permissions(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o600));
        }
    }
}

#[cfg(not(unix))]
fn harden_log_permissions(_dir: &Path) {}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use tempfile::tempdir;

    use super::*;

    /// Stefans Fund: `.log` muss am Ende des Dateinamens stehen (Datei-
    /// zuordnung im Betriebssystem hängt daran), nicht in der Mitte vor
    /// dem Datum. Baut denselben `Builder`-Aufruf wie `init_logging`
    /// direkt (statt `init_logging()` selbst aufzurufen — das würde den
    /// GLOBALEN `tracing`-Subscriber setzen, was in einer Testsuite mit
    /// mehreren Tests nur einmal möglich ist und mit anderen Tests
    /// kollidieren würde) und schreibt eine Zeile, um den tatsächlich
    /// erzeugten Dateinamen zu sehen.
    #[test]
    fn test_rolling_log_file_name_ends_with_dot_log_not_the_date() {
        use std::io::Write;

        let dir = tempdir().unwrap();
        let mut appender = tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix(LOG_FILE_PREFIX)
            .filename_suffix(LOG_FILE_SUFFIX)
            .build(dir.path())
            .unwrap();
        appender.write_all(b"line\n").unwrap();

        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            names.len(),
            1,
            "erwartet genau eine erzeugte Datei: {names:?}"
        );
        assert!(
            names[0].ends_with(".log"),
            "Dateiname muss mit .log enden, nicht das Datum danach haben: {}",
            names[0]
        );
        assert!(
            names[0].starts_with("smart-ssh."),
            "Präfix muss weiterhin vorne stehen: {}",
            names[0]
        );
    }

    /// Simuliert Alter über ein weit in der Zukunft liegendes `now` statt
    /// über manipuliertes `mtime` — deckt denselben Pfad wie
    /// `test_cleanup_old_logs_keeps_recent_and_removes_old_file_side_by_side`
    /// mit einer unabhängigen Technik ab (kein `File::set_modified`
    /// beteiligt, das auf manchen Dateisystemen/Plattformen Einschränkungen
    /// haben könnte).
    #[test]
    fn test_cleanup_old_logs_removes_file_when_now_is_far_in_the_future() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("smart-ssh.log.2020-01-01");
        File::create(&path).unwrap();

        let far_future = SystemTime::now() + Duration::from_secs(20 * 24 * 60 * 60);
        cleanup_old_logs(
            dir.path(),
            Duration::from_secs(14 * 24 * 60 * 60),
            far_future,
        )
        .unwrap();

        assert!(!path.exists());
    }

    /// Unabhängiger Review-Pass (Spec 0016): Log-Ordner/-Dateien müssen
    /// dieselbe 0700/0600-Beschränkung bekommen wie die SQLite-DB und
    /// `host_keys.json` (s. `store::tests`/`host_key_store::tests::
    /// test_t10_posix_permissions_enforced` für dasselbe Testmuster).
    #[cfg(unix)]
    #[test]
    fn test_harden_log_permissions_sets_0700_dir_and_0600_files() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let file_path = dir.path().join("smart-ssh.log.today");
        File::create(&file_path).unwrap();

        harden_log_permissions(dir.path());

        let dir_mode = fs::metadata(dir.path()).unwrap().permissions().mode();
        assert_eq!(dir_mode & 0o777, 0o700);
        let file_mode = fs::metadata(&file_path).unwrap().permissions().mode();
        assert_eq!(file_mode & 0o777, 0o600);
    }

    #[test]
    fn test_cleanup_old_logs_keeps_file_when_within_max_age() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("smart-ssh.log.today");
        File::create(&path).unwrap();

        let now = SystemTime::now();
        cleanup_old_logs(dir.path(), Duration::from_secs(14 * 24 * 60 * 60), now).unwrap();

        assert!(path.exists());
    }

    #[test]
    fn test_cleanup_old_logs_keeps_recent_and_removes_old_file_side_by_side() {
        let dir = tempdir().unwrap();
        let old_path = dir.path().join("smart-ssh.log.old");
        let recent_path = dir.path().join("smart-ssh.log.recent");
        File::create(&old_path).unwrap();
        File::create(&recent_path).unwrap();

        let now = SystemTime::now();
        // Alte Datei: mtime auf vor 20 Tagen zurückgesetzt.
        File::options()
            .write(true)
            .open(&old_path)
            .unwrap()
            .set_modified(now - Duration::from_secs(20 * 24 * 60 * 60))
            .unwrap();
        // Neuere Datei: mtime auf vor 1 Tag.
        File::options()
            .write(true)
            .open(&recent_path)
            .unwrap()
            .set_modified(now - Duration::from_secs(24 * 60 * 60))
            .unwrap();

        cleanup_old_logs(dir.path(), Duration::from_secs(14 * 24 * 60 * 60), now).unwrap();

        assert!(
            !old_path.exists(),
            "Datei älter als 14 Tage muss gelöscht werden"
        );
        assert!(
            recent_path.exists(),
            "Datei jünger als 14 Tage muss erhalten bleiben"
        );
    }

    #[test]
    fn test_cleanup_old_logs_on_missing_directory_is_a_no_op() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");

        let result = cleanup_old_logs(&missing, MAX_LOG_AGE, SystemTime::now());

        assert!(result.is_ok());
    }

    #[test]
    fn test_read_last_log_lines_returns_only_the_tail() {
        let dir = tempdir().unwrap();
        let lines: Vec<String> = (1..=10).map(|n| format!("line {n}")).collect();
        std::fs::write(
            dir.path().join("smart-ssh.2026-01-01.log"),
            lines.join("\n"),
        )
        .unwrap();

        let result = read_last_log_lines(dir.path(), 3);

        assert_eq!(result, vec!["line 8", "line 9", "line 10"]);
    }

    #[test]
    fn test_read_last_log_lines_returns_all_lines_when_fewer_than_max() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("smart-ssh.2026-01-01.log"), "a\nb").unwrap();

        let result = read_last_log_lines(dir.path(), 500);

        assert_eq!(result, vec!["a", "b"]);
    }

    #[test]
    fn test_read_last_log_lines_on_missing_directory_returns_empty_instead_of_erroring() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");

        let result = read_last_log_lines(&missing, 500);

        assert!(result.is_empty());
    }

    /// Spec 0063, Teil 3: bei mehreren Log-Dateien (z. B. nach einem
    /// Tageswechsel) muss die zuletzt geschriebene gewählt werden, nicht
    /// irgendeine/die alphabetisch letzte.
    #[test]
    fn test_read_last_log_lines_picks_the_most_recently_modified_file() {
        let dir = tempdir().unwrap();
        let older = dir.path().join("smart-ssh.2026-01-01.log");
        let newer = dir.path().join("smart-ssh.2026-01-02.log");
        std::fs::write(&older, "old content").unwrap();
        std::fs::write(&newer, "new content").unwrap();
        let now = SystemTime::now();
        File::options()
            .write(true)
            .open(&older)
            .unwrap()
            .set_modified(now - Duration::from_secs(60))
            .unwrap();
        File::options()
            .write(true)
            .open(&newer)
            .unwrap()
            .set_modified(now)
            .unwrap();

        let result = read_last_log_lines(dir.path(), 10);

        assert_eq!(result, vec!["new content"]);
    }

    /// Spec-reviewer-Fund (Follow-up-Review, Spec 0063): eine Fremddatei im
    /// Log-Ordner mit neuerer `mtime` als die eigentliche Log-Datei darf
    /// nicht ausgewählt werden — sonst würde ihr (unbekannter, ggf.
    /// sensibler) Inhalt ungeprüft ins öffentlich geteilte Diagnosepaket
    /// wandern.
    #[test]
    fn test_read_last_log_lines_ignores_newer_non_log_file() {
        let dir = tempdir().unwrap();
        let log_file = dir.path().join("smart-ssh.2026-01-01.log");
        let foreign_file = dir.path().join("some-other-artifact.txt");
        std::fs::write(&log_file, "actual log content").unwrap();
        std::fs::write(&foreign_file, "unrelated, possibly sensitive content").unwrap();
        let now = SystemTime::now();
        File::options()
            .write(true)
            .open(&log_file)
            .unwrap()
            .set_modified(now - Duration::from_secs(60))
            .unwrap();
        File::options()
            .write(true)
            .open(&foreign_file)
            .unwrap()
            .set_modified(now)
            .unwrap();

        let result = read_last_log_lines(dir.path(), 10);

        assert_eq!(result, vec!["actual log content"]);
    }
}
