//! Issue #19: Die Sperre auf das Datenverzeichnis, geprüft mit einem
//! **echten zweiten Prozess** als Halter.
//!
//! Der Halter ist dieses Test-Binary selbst: [`LockHolder::spawn`] startet
//! es mit genau dem Test [`lock_holder_child_process`], der die Sperre
//! nimmt, eine Bereitschaftszeile schreibt und dann bis zum Ende seiner
//! Standardeingabe wartet. Damit gilt, was in der App gilt: zwei Prozesse,
//! eine Sperrdatei, und die Freigabe durch das Betriebssystem beim
//! Prozessende (auch nach `kill`).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use persistence_sqlite::{
    detect_database_file_state, ConnectFailureKind, DataDirLock, DatabaseFileState,
};

use super::tests::{
    available, plaintext_database, CountingCredentialStore, GetBehaviour, NoDialogExpected,
};
use super::*;
use crate::startup_error_messages::{db_connect_failure_text, Language};

/// Woran der Kindprozess sein Verzeichnis erkennt. Ohne die Variable tut
/// der Hilfstest nichts (etwa bei `cargo test -- --ignored`).
const HOLDER_DIR_ENV: &str = "SMART_SSH_TEST_LOCK_HOLDER_DIR";
/// Die Zeile, mit der der Kindprozess meldet, dass er die Sperre hält.
const HOLDER_READY: &str = "issue-19-lock-holder-ready";
/// Voller Name des Hilfstests im Test-Binary dieser Crate.
const HOLDER_TEST: &str = "database_startup::instance_lock_tests::lock_holder_child_process";

/// Läuft nur als Kindprozess von [`LockHolder::spawn`].
#[test]
#[ignore = "helper: runs only as the child process spawned by LockHolder"]
fn lock_holder_child_process() {
    let Some(dir) = std::env::var_os(HOLDER_DIR_ENV) else {
        return;
    };
    let _lock = DataDirLock::acquire(Path::new(&dir)).expect("child: acquire the lock");
    let mut stdout = std::io::stdout();
    writeln!(stdout, "\n{HOLDER_READY}").unwrap();
    stdout.flush().unwrap();
    // Hält die Sperre, bis der Elternprozess die Eingabe schließt oder
    // den Prozess beendet.
    let mut sink = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut sink);
}

/// Ein zweiter Prozess, der die Sperre auf ein Verzeichnis hält.
struct LockHolder {
    child: Option<Child>,
}

impl LockHolder {
    fn spawn(dir: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().expect("test binary path"))
            .args([
                HOLDER_TEST,
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(HOLDER_DIR_ENV, dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn the lock-holding child process");

        // Echtes Signal statt Wartezeit: Der Kindprozess meldet sich, wenn
        // er die Sperre hält. Endet seine Ausgabe vorher, ist er gescheitert.
        let stdout = child.stdout.take().expect("child stdout");
        let (ready_tx, ready_rx) = mpsc::channel::<bool>();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.contains(HOLDER_READY) {
                    let _ = ready_tx.send(true);
                }
            }
            let _ = ready_tx.send(false);
        });
        let mut holder = Self { child: Some(child) };
        match ready_rx.recv_timeout(Duration::from_secs(60)) {
            Ok(true) => holder,
            other => {
                holder.kill();
                panic!("the child process did not report holding the lock: {other:?}");
            }
        }
    }

    /// Beendet den Halter hart (wie ein abgestürzter oder per `kill -9`
    /// beendeter Prozess) und wartet, bis er weg ist.
    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for LockHolder {
    fn drop(&mut self) {
        self.kill();
    }
}

fn assert_already_running(result: Result<DataDirLock, StartupAbort>) {
    match result {
        Err(StartupAbort::Fatal { kind, .. }) => {
            assert_eq!(kind, ConnectFailureKind::AlreadyRunning)
        }
        Err(other) => panic!("expected AlreadyRunning, got {other:?}"),
        Ok(lock) => panic!("the lock must not be acquirable, got {lock:?}"),
    }
}

/// AC: Mit der Sperre bei einem anderen Prozess endet der Start mit dem
/// Startfehler „läuft bereits", und die Datenbankdatei ist unverändert.
#[tokio::test(flavor = "multi_thread")]
async fn test_issue_19_a_lock_held_by_another_process_stops_the_start_and_leaves_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = plaintext_database(dir.path()).await;
    let before = std::fs::read(&db_path).unwrap();
    let _holder = LockHolder::spawn(dir.path());

    assert_already_running(lock_data_directory(&db_path));

    assert_eq!(std::fs::read(&db_path).unwrap(), before, "file changed");
    assert_eq!(
        detect_database_file_state(&db_path).unwrap(),
        DatabaseFileState::Plaintext
    );
    assert!(!persistence_sqlite::intermediate_path(&db_path).exists());
}

/// Der Dialog zum Fall nennt die Ursache wörtlich (Issue #19), in beiden
/// Sprachen, und rät **nicht** zu einem Backup — es ist nichts beschädigt.
#[test]
fn test_issue_19_the_already_running_dialog_says_so_in_both_languages() {
    let db = Path::new("/tmp/test/smart-ssh.db");
    let logs = Path::new("/tmp/test/logs");
    let en = db_connect_failure_text(&ConnectFailureKind::AlreadyRunning, db, logs, Language::En);
    let de = db_connect_failure_text(&ConnectFailureKind::AlreadyRunning, db, logs, Language::De);

    assert!(
        en.message
            .contains("Smart SSH is already running with this data directory"),
        "{}",
        en.message
    );
    assert!(
        de.message
            .contains("Smart SSH läuft bereits mit diesem Datenverzeichnis"),
        "{}",
        de.message
    );
    for text in [&en, &de] {
        assert!(
            text.message.contains("/tmp/test/smart-ssh.db"),
            "{}",
            text.message
        );
        assert!(!text.message.contains("Backup"), "{}", text.message);
    }
}

/// AC: Nachdem der haltende Prozess beendet wurde (hart, ohne Aufräumen),
/// gelingt ein neuer Start — die Sperrdatei bleibt liegen, blockiert aber
/// nicht, weil das Betriebssystem die Sperre mit dem Prozess freigibt.
#[tokio::test(flavor = "multi_thread")]
async fn test_issue_19_after_the_holder_is_killed_a_new_start_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = plaintext_database(dir.path()).await;
    let mut holder = LockHolder::spawn(dir.path());
    assert_already_running(lock_data_directory(&db_path));

    holder.kill();

    // Windows gibt die Sperre eines beendeten Prozesses laut
    // `LockFileEx`-Dokumentation nicht zwingend sofort frei, sondern „je
    // nach verfügbaren Systemressourcen". Deshalb ein begrenztes erneutes
    // Versuchen statt eines einzigen; auf macOS/Linux gelingt schon der
    // erste Versuch.
    let deadline = Instant::now() + Duration::from_secs(10);
    let lock = loop {
        match lock_data_directory(&db_path) {
            Ok(lock) => break lock,
            Err(err) if Instant::now() < deadline => {
                drop(err);
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => panic!("the lock of a killed process must be released: {err:?}"),
        }
    };
    assert!(
        dir.path()
            .join(persistence_sqlite::DATA_DIR_LOCK_FILE_NAME)
            .exists(),
        "the leftover lock file is expected to stay — and must not block"
    );

    // Und der Start läuft tatsächlich durch, einschließlich der Umwandlung.
    let credentials = CountingCredentialStore::new(GetBehaviour::Present);
    open_or_prepare_database(
        &db_path,
        RootKeyAccess::Keychain(&credentials),
        available(),
        &NoDialogExpected,
        &lock,
    )
    .await
    .expect("with the lock released, the start must succeed");
    assert_eq!(
        detect_database_file_state(&db_path).unwrap(),
        DatabaseFileState::Other
    );
}

/// AC: Die Umwandlung (A6) kann nicht laufen, solange ein anderer Prozess
/// die Sperre hält.
///
/// Die Ausgangslage ist genau die, in der der Start umwandeln würde
/// (Klartext-Datei, K vorhanden). Dieser Prozess bekommt die Sperre nicht
/// — und ohne sie gibt es keinen Weg in die Umwandlung: Auch mit der
/// einzigen Sperre, die er nehmen kann (auf ein anderes Verzeichnis),
/// endet der Start, bevor etwas geöffnet ist, und die Datei bleibt
/// bytegleich im Klartext.
#[tokio::test(flavor = "multi_thread")]
async fn test_issue_19_the_a6_conversion_cannot_run_while_another_process_holds_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = plaintext_database(dir.path()).await;
    let before = std::fs::read(&db_path).unwrap();
    let _holder = LockHolder::spawn(dir.path());

    assert_already_running(lock_data_directory(&db_path));

    let elsewhere = tempfile::tempdir().unwrap();
    let foreign_lock = DataDirLock::acquire(elsewhere.path()).unwrap();
    let credentials = CountingCredentialStore::new(GetBehaviour::Present);
    let result = open_or_prepare_database(
        &db_path,
        RootKeyAccess::Keychain(&credentials),
        available(),
        &NoDialogExpected,
        &foreign_lock,
    )
    .await;

    match result {
        Err(StartupAbort::Fatal { kind, detail }) => {
            assert_eq!(kind, ConnectFailureKind::ConversionFailed, "{detail}");
        }
        Err(other) => panic!("expected a fatal start error, got {other:?}"),
        Ok(_) => panic!("the conversion must not run without the lock"),
    }
    assert_eq!(std::fs::read(&db_path).unwrap(), before, "file changed");
    assert_eq!(
        detect_database_file_state(&db_path).unwrap(),
        DatabaseFileState::Plaintext
    );
    assert!(!persistence_sqlite::intermediate_path(&db_path).exists());
}
