//! Issue #19: genau ein Prozess arbeitet auf einem Datenverzeichnis.
//!
//! Eine exklusive Sperre auf der Datei `smart-ssh.lock` im
//! Datenverzeichnis, genommen **bevor** die Datenbank geöffnet, migriert
//! oder umgewandelt wird, und gehalten, solange der Prozess läuft.
//!
//! **Betriebssystem-Sperre, keine PID-Datei:** `std::fs::File::try_lock`
//! (`flock` auf macOS/Linux, `LockFileEx` auf Windows). Das Betriebssystem
//! gibt die Sperre frei, sobald der Prozess endet — auch bei einem Absturz
//! oder `kill -9`. Eine liegengebliebene Sperrdatei blockiert deshalb nie
//! einen Start; nur ein **laufender** Prozess, der sie hält, tut das. Der
//! Inhalt der Datei ist bedeutungslos, sie wird nie gelöscht.
//!
//! Die Sperre deckt auch zwei **verschiedene** Builds ab, die sich ein
//! Datenverzeichnis teilen (z. B. Community und eine andere Edition, oder
//! zwei Versionen) — anders als der Single-Instance-Schutz in `app-shell`,
//! der nur Instanzen derselben App-Kennung erkennt.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// Dateiname der Sperrdatei im Datenverzeichnis.
pub const DATA_DIR_LOCK_FILE_NAME: &str = "smart-ssh.lock";

/// Warum die Sperre nicht genommen werden konnte.
#[derive(Debug)]
pub enum DataDirLockError {
    /// Ein anderer Prozess hält die Sperre — eine laufende Instanz (oder
    /// ein anderer Build) arbeitet auf diesem Datenverzeichnis.
    HeldByAnotherProcess,
    /// Datenverzeichnis oder Sperrdatei ließen sich nicht anlegen/öffnen,
    /// oder das Sperren selbst schlug fehl. **Kein Weiterlaufen ohne
    /// Sperre** — der Aufrufer bricht den Start ab.
    Io(std::io::Error),
}

impl std::fmt::Display for DataDirLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeldByAnotherProcess => {
                write!(f, "the data directory is locked by another process")
            }
            Self::Io(err) => write!(f, "could not lock the data directory: {err}"),
        }
    }
}

impl std::error::Error for DataDirLockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::HeldByAnotherProcess => None,
            Self::Io(err) => Some(err),
        }
    }
}

/// Die gehaltene Sperre. Solange ein Wert lebt, kann kein anderer Prozess
/// (und kein zweites `acquire` in diesem Prozess) dasselbe Verzeichnis
/// sperren. Mit dem `Drop` endet die Sperre; beim Prozessende ohnehin.
///
/// Wer die Datenbank umwandelt, braucht einen Verweis darauf
/// ([`crate::convert_plaintext_database`]) — die Umwandlung lässt sich
/// damit gar nicht erst aufrufen, ohne dass dieser Prozess die Sperre hält.
#[derive(Debug)]
pub struct DataDirLock {
    /// Kanonischer Pfad des gesperrten Verzeichnisses.
    dir: PathBuf,
    /// Hält die Sperre. Nie gelesen; das Schließen gibt sie frei.
    _file: File,
}

impl DataDirLock {
    /// Sperrt `data_dir` exklusiv, ohne zu warten. Legt das Verzeichnis und
    /// die Sperrdatei an, falls sie fehlen — sonst nichts.
    pub fn acquire(data_dir: &Path) -> Result<Self, DataDirLockError> {
        std::fs::create_dir_all(data_dir).map_err(DataDirLockError::Io)?;
        let dir = data_dir.canonicalize().map_err(DataDirLockError::Io)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            // Der Inhalt ist bedeutungslos; nicht kürzen, damit das Öffnen
            // nichts an einer Datei ändert, die ein anderer Prozess hält.
            .truncate(false)
            .open(dir.join(DATA_DIR_LOCK_FILE_NAME))
            .map_err(DataDirLockError::Io)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { dir, _file: file }),
            Err(TryLockError::WouldBlock) => Err(DataDirLockError::HeldByAnotherProcess),
            Err(TryLockError::Error(err)) => Err(DataDirLockError::Io(err)),
        }
    }

    /// Sperrt das Verzeichnis, in dem `db_path` liegt.
    pub fn acquire_for_database(db_path: &Path) -> Result<Self, DataDirLockError> {
        Self::acquire(database_dir(db_path))
    }

    /// Das gesperrte Verzeichnis (kanonisch).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Liegt `db_path` direkt in dem gesperrten Verzeichnis?
    ///
    /// **Schlägt im Zweifel fehl** (`false`): Lässt sich das Verzeichnis
    /// von `db_path` nicht kanonisieren, gilt es als nicht gedeckt.
    pub fn covers_database(&self, db_path: &Path) -> bool {
        database_dir(db_path)
            .canonicalize()
            .is_ok_and(|dir| dir == self.dir)
    }
}

/// Das Verzeichnis, in dem die Datenbankdatei liegt. Ein bloßer Dateiname
/// liegt im aktuellen Verzeichnis.
fn database_dir(db_path: &Path) -> &Path {
    match db_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_creates_the_directory_and_the_lock_file() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("not-yet-there");

        let lock = DataDirLock::acquire(&data_dir).unwrap();

        assert!(data_dir.join(DATA_DIR_LOCK_FILE_NAME).is_file());
        assert_eq!(lock.dir(), data_dir.canonicalize().unwrap());
    }

    /// Eine zweite Sperre auf demselben Verzeichnis scheitert, solange die
    /// erste gehalten wird — `flock`/`LockFileEx` gelten je geöffneter
    /// Datei, ein zweites Öffnen im selben Prozess verhält sich also wie
    /// ein zweiter Prozess. (Der Nachweis mit einem echten zweiten Prozess
    /// steht in `app_logic::instance_lock`.)
    #[test]
    fn a_second_lock_on_the_same_directory_is_refused_while_the_first_is_held() {
        let tmp = tempfile::tempdir().unwrap();
        let _first = DataDirLock::acquire(tmp.path()).unwrap();

        let second = DataDirLock::acquire(tmp.path());

        assert!(
            matches!(second, Err(DataDirLockError::HeldByAnotherProcess)),
            "{second:?}"
        );
    }

    #[test]
    fn dropping_the_lock_releases_it() {
        let tmp = tempfile::tempdir().unwrap();
        drop(DataDirLock::acquire(tmp.path()).unwrap());

        DataDirLock::acquire(tmp.path()).expect("released lock must be acquirable again");
    }

    /// Eine liegengebliebene Sperrdatei (mit beliebigem Inhalt, etwa einer
    /// alten PID) blockiert nichts — nur eine gehaltene Sperre tut das.
    #[test]
    fn a_leftover_lock_file_without_a_holder_does_not_block() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(DATA_DIR_LOCK_FILE_NAME), b"12345\n").unwrap();

        DataDirLock::acquire(tmp.path()).expect("an unheld lock file must not block");
        assert_eq!(
            std::fs::read(tmp.path().join(DATA_DIR_LOCK_FILE_NAME)).unwrap(),
            b"12345\n",
            "acquiring must not rewrite the lock file"
        );
    }

    #[test]
    fn different_directories_lock_independently() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let _a = DataDirLock::acquire(a.path()).unwrap();

        DataDirLock::acquire(b.path()).expect("another directory must stay lockable");
    }

    #[test]
    fn covers_only_databases_in_the_locked_directory() {
        let locked = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let lock = DataDirLock::acquire_for_database(&locked.path().join("smart-ssh.db")).unwrap();

        assert!(lock.covers_database(&locked.path().join("smart-ssh.db")));
        // Nicht kanonisch geschrieben, aber dasselbe Verzeichnis.
        assert!(lock.covers_database(&locked.path().join(".").join("smart-ssh.db")));
        assert!(!lock.covers_database(&other.path().join("smart-ssh.db")));
        // Unterverzeichnis ist ein anderes Verzeichnis.
        std::fs::create_dir(locked.path().join("sub")).unwrap();
        assert!(!lock.covers_database(&locked.path().join("sub").join("smart-ssh.db")));
        // Nicht existierendes Verzeichnis: im Zweifel nicht gedeckt.
        assert!(!lock.covers_database(&locked.path().join("missing").join("smart-ssh.db")));
    }
}
