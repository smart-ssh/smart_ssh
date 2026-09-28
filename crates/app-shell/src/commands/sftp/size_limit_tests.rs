//! Spec 0086, A1 (T1–T7): Die Größengrenzen von „Dateiinhalt kopieren" und
//! „Lokal öffnen" halten auch, wenn die Datei zwischen `stat` und `read_file`
//! wächst — oder `stat` gar nichts liefert.
//!
//! Angriffsbild: Beide Befehle prüften die Größe nur per `stat` und lasen
//! danach mit `read_file`. Die Kanal-Sperre wird seit Spec 0085 (A1.3) je
//! Operation genommen (ADR 0078 §1), zwischen den beiden Aufrufen kann also
//! ein anderer Befehl laufen. Wuchs die Datei in dieser Lücke — oder
//! scheiterte `stat`, was das Lesen bewusst nicht blockiert —, wurde sie
//! vollständig gelesen und ausgeliefert. „Lokal öffnen" hatte überhaupt keine
//! Grenze und schrieb jede Größe ins Editier-Temp-Verzeichnis.
//!
//! Alle Tests fahren den **echten** Befehlsrumpf (`read_text_impl`,
//! `open_for_editing_impl`) durch den **echten** gemeinsamen Rahmen
//! (`with_browser_channel`), wie `revocation_tests` — nur das Auflösen von
//! Sitzung und Kanal aus dem `AppState` fehlt, weil ein Tauri-`State` im
//! Unit-Test nicht baubar ist.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;

use ssh_manager_core::ssh::{
    CommandOutput, InteractiveShell, PtySize, RemoteEntry, SftpSession, SshError, SshTransport,
};

use app_logic::session::{Session, SessionManager};
use app_logic::state::SessionId;

use super::*;
use crate::commands::elevation::{with_browser_channel, BrowserChannel};
use crate::elevated_sftp::{ElevatedSftp, ElevatedSftpRegistry};

// --- Test-Double: `stat` und `read_file` dürfen sich widersprechen -----------

/// Was `stat` über die Datei behauptet.
#[derive(Clone)]
enum StatResult {
    /// `stat` gelingt und meldet diese Größe.
    Size(u64),
    /// `stat` scheitert — laut Doc-Kommentar von `sftp_read_text` blockiert
    /// das das Lesen bewusst nicht.
    Fails,
}

/// Ein SFTP-Kanal, dessen `stat`-Größe und `read_file`-Inhalt sich frei
/// widersprechen dürfen — genau die Lücke, die A1 schließt. Zählt die
/// `read_file`-Aufrufe, damit T4 belegen kann, dass gar nicht gelesen wurde.
#[derive(Clone)]
struct MismatchSftp(Arc<Mismatch>);

struct Mismatch {
    stat: StatResult,
    content: Vec<u8>,
    read_calls: AtomicUsize,
    ops: StdMutex<Vec<String>>,
}

impl MismatchSftp {
    fn new(stat: StatResult, content: Vec<u8>) -> Self {
        Self(Arc::new(Mismatch {
            stat,
            content,
            read_calls: AtomicUsize::new(0),
            ops: StdMutex::new(Vec::new()),
        }))
    }

    /// `stat` meldet `claimed_size`, `read_file` liefert `actual_len` Bytes
    /// gültiges UTF-8 (`b'x'`).
    fn claiming(claimed_size: u64, actual_len: usize) -> Self {
        Self::new(StatResult::Size(claimed_size), vec![b'x'; actual_len])
    }

    fn read_calls(&self) -> usize {
        self.0.read_calls.load(Ordering::SeqCst)
    }

    fn ops(&self) -> Vec<String> {
        self.0.ops.lock().unwrap().clone()
    }
}

#[async_trait]
impl SftpSession for MismatchSftp {
    async fn list_dir(&mut self, _path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        unreachable!("diese Tests listen kein Verzeichnis")
    }

    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.0.read_calls.fetch_add(1, Ordering::SeqCst);
        self.0.ops.lock().unwrap().push(format!("read_file {path}"));
        Ok(self.0.content.clone())
    }

    async fn write_file(&mut self, _path: &str, _content: &[u8]) -> Result<(), SshError> {
        unreachable!("diese Tests schreiben nichts auf den Server")
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.0.ops.lock().unwrap().push(format!("stat {path}"));
        match self.0.stat {
            StatResult::Size(size) => Ok(RemoteEntry {
                name: path.rsplit('/').next().unwrap_or(path).to_string(),
                path: path.to_string(),
                is_dir: false,
                size,
                permissions: 0o644,
                modified: None,
                uid: None,
                gid: None,
                owner: None,
                group: None,
            }),
            StatResult::Fails => Err(SshError::ChannelError("stat nicht möglich".to_string())),
        }
    }

    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.stat(path).await
    }

    async fn remove(&mut self, _path: &str) -> Result<(), SshError> {
        unreachable!("diese Tests löschen nichts")
    }

    async fn rename(&mut self, _from: &str, _to: &str) -> Result<(), SshError> {
        unreachable!("diese Tests benennen nichts um")
    }

    async fn create_dir(&mut self, _path: &str) -> Result<(), SshError> {
        unreachable!("diese Tests legen kein Verzeichnis an")
    }

    async fn remove_dir(&mut self, _path: &str) -> Result<(), SshError> {
        unreachable!("diese Tests löschen kein Verzeichnis")
    }

    async fn set_permissions(&mut self, _path: &str, _mode: u32) -> Result<(), SshError> {
        unreachable!("diese Tests ändern keine Rechte")
    }
}

struct NoTransport;
#[async_trait]
impl SshTransport for NoTransport {
    async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
        unreachable!("diese Tests führen kein Kommando aus")
    }
    async fn open_shell(&mut self, _size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        unreachable!()
    }
    async fn disconnect(&mut self) -> Result<(), SshError> {
        Ok(())
    }
}

// --- Aufbau ------------------------------------------------------------------

/// Sitzung und beide Kanäle. Der **erhöhte** Kanal ist immer eingetragen,
/// damit dieselbe Sitzung beide Wege aus A1.4 abdeckt; welcher benutzt wird,
/// entscheidet der Kanal, den der Test mitgibt.
struct Setup {
    registry: Arc<ElevatedSftpRegistry>,
    session_id: SessionId,
    session: Arc<Session>,
}

async fn setup(normal: MismatchSftp, elevated: MismatchSftp) -> Setup {
    let session = Arc::new(app_logic::test_support::session_with_transport(Box::new(
        NoTransport,
    )));
    session.set_sftp_for_tests(Box::new(normal)).await;

    let sessions = Arc::new(SessionManager::new());
    let session_id = SessionId::new_v4();
    sessions.insert(session_id, session.clone());

    let registry = Arc::new(ElevatedSftpRegistry::default());
    registry.insert_for_tests(
        session_id,
        ElevatedSftp {
            target_user: "root".to_string(),
            sftp: Box::new(elevated),
        },
    );

    Setup {
        registry,
        session_id,
        session,
    }
}

impl Setup {
    fn normal_channel(&self) -> BrowserChannel {
        BrowserChannel::for_tests(&self.registry, self.session_id, None)
    }

    fn elevated_channel(&self) -> BrowserChannel {
        BrowserChannel::for_tests(&self.registry, self.session_id, Some("root".to_string()))
    }
}

/// Ein Kanal, der nie angefasst werden darf — belegt in jedem Test, dass die
/// Grenze auf dem benutzten Kanal greift und nicht versehentlich auf dem
/// anderen.
fn untouched() -> MismatchSftp {
    MismatchSftp::claiming(0, 0)
}

/// Spec 0086, A1.1: die Ablehnung ist **wörtlich** dieselbe, egal ob sie aus
/// der `stat`-Vorabprüfung oder aus der Prüfung nach dem Lesen kommt.
const TOO_LARGE_FOR_CLIPBOARD: &str =
    "Datei ist größer als 256 KB — zu groß zum Kopieren in die Zwischenablage";

fn assert_too_large_for_clipboard(result: CommandResult<String>) {
    let err = result.expect_err("eine zu große Datei darf nicht ausgeliefert werden");
    assert_eq!(
        err.message, TOO_LARGE_FOR_CLIPBOARD,
        "die Meldung muss wörtlich die der `stat`-Ablehnung sein (A1.1)"
    );
}

fn assert_too_large_for_editing(result: CommandResult<EditSessionDto>) {
    let err = result.expect_err("eine zu große Datei darf nicht lokal geöffnet werden");
    assert_eq!(
        err.message,
        "Datei ist größer als 50 MB — zu groß zum lokalen Öffnen. Bitte stattdessen herunterladen.",
        "die Meldung muss wörtlich die aus A1.2 sein"
    );
}

// --- T1–T3: „Dateiinhalt kopieren" (A1.1) -----------------------------------

/// Spec 0086, T1: `stat` meldet 100 Bytes, `read_file` liefert 256 KB + 1.
/// Vor dem Fix kam der ganze Inhalt zurück.
#[tokio::test]
async fn test_t1_a_file_that_grew_between_stat_and_read_is_rejected_after_reading() {
    let normal = MismatchSftp::claiming(100, (MAX_TEXT_PREVIEW_BYTES + 1) as usize);
    let s = setup(normal.clone(), untouched()).await;

    let result = with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move { read_text_impl(&session, &channel, "/t/grown.txt").await },
    )
    .await;

    assert_too_large_for_clipboard(result);
    assert_eq!(
        normal.ops(),
        vec![
            "stat /t/grown.txt".to_string(),
            "read_file /t/grown.txt".to_string()
        ],
        "die Vorabprüfung per `stat` bleibt, sie bestand hier nur"
    );
}

/// Spec 0086, T2: `stat` scheitert (blockiert das Lesen bewusst nicht),
/// `read_file` liefert 256 KB + 1 — dieselbe Ablehnung, derselbe Text.
#[tokio::test]
async fn test_t2_a_failing_stat_no_longer_lets_an_oversized_file_through() {
    let normal = MismatchSftp::new(
        StatResult::Fails,
        vec![b'x'; (MAX_TEXT_PREVIEW_BYTES + 1) as usize],
    );
    let s = setup(normal.clone(), untouched()).await;

    let result =
        with_browser_channel(
            s.session.clone(),
            s.normal_channel(),
            |session, channel| async move {
                read_text_impl(&session, &channel, "/t/unstattable.txt").await
            },
        )
        .await;

    assert_too_large_for_clipboard(result);
    assert_eq!(
        normal.read_calls(),
        1,
        "ein gescheitertes `stat` blockiert das Lesen weiter nicht — nur das \
         Ergebnis wird jetzt geprüft"
    );
}

/// Spec 0086, T3: Absicherung gegen ein `>=` statt `>` — genau 256 KB
/// gültiges UTF-8 kommt weiter durch.
#[tokio::test]
async fn test_t3_exactly_the_limit_is_still_returned() {
    let content = vec![b'x'; MAX_TEXT_PREVIEW_BYTES as usize];
    let normal = MismatchSftp::new(StatResult::Size(MAX_TEXT_PREVIEW_BYTES), content.clone());
    let s = setup(normal, untouched()).await;

    let text = with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move { read_text_impl(&session, &channel, "/t/exact.txt").await },
    )
    .await
    .expect("genau 256 KB liegen noch innerhalb der Grenze");

    assert_eq!(text.len(), MAX_TEXT_PREVIEW_BYTES as usize);
}

// --- T4–T6: „Lokal öffnen" (A1.2/A1.3) --------------------------------------

/// Öffnet lokal und räumt das Editier-Verzeichnis danach wieder auf. Gibt
/// zusätzlich das Verzeichnis zurück, damit der Test prüfen kann, ob dort
/// etwas entstanden ist.
async fn open_for_editing(
    s: &Setup,
    channel: BrowserChannel,
    remote_path: &'static str,
    max_bytes: u64,
) -> (CommandResult<EditSessionDto>, std::path::PathBuf) {
    let session_id = s.session_id;
    let result = with_browser_channel(
        s.session.clone(),
        channel,
        move |session, channel| async move {
            open_for_editing_impl(&session, &channel, session_id, remote_path, max_bytes).await
        },
    )
    .await;
    (result, edit_session_dir(session_id).unwrap())
}

/// Spec 0086, T4: `stat` meldet 50 MB + 1 — abgelehnt **vor** dem Lesen, und
/// im Editier-Temp-Verzeichnis entsteht nichts.
#[tokio::test]
async fn test_t4_a_file_stat_already_reports_as_too_large_is_never_read() {
    let normal = MismatchSftp::claiming(MAX_EDIT_OPEN_BYTES + 1, 0);
    let s = setup(normal.clone(), untouched()).await;

    let (result, dir) =
        open_for_editing(&s, s.normal_channel(), "/t/huge.bin", MAX_EDIT_OPEN_BYTES).await;

    assert_too_large_for_editing(result);
    assert_eq!(
        normal.read_calls(),
        0,
        "eine per `stat` erkennbar zu große Datei darf nicht erst übertragen werden"
    );
    assert!(
        !dir.exists(),
        "bei einer Ablehnung entsteht kein Editier-Verzeichnis: {dir:?}"
    );
}

/// Spec 0086, T5/A1.3: `stat` meldet 10 Bytes, gelesen wird mehr als erlaubt
/// — abgelehnt **nach** dem Lesen, ohne lokale Datei. Eine dort schon
/// liegende Kopie aus einem früheren „Lokal öffnen" bleibt byte-gleich.
///
/// Die Grenze kommt als Parameter (T5 erlaubt das ausdrücklich, s. ADR 0080):
/// ein echter 50-MB-Puffer je Fall wäre reine Laufzeit ohne zusätzliche
/// Aussage — geprüft wird der Vergleich, nicht die Zahl.
#[tokio::test]
async fn test_t5_a_file_that_grew_between_stat_and_read_leaves_no_local_copy() {
    const LIMIT: u64 = 64;
    let normal = MismatchSftp::claiming(10, LIMIT as usize + 1);
    let s = setup(normal.clone(), untouched()).await;

    // Eine Kopie aus einem früheren „Lokal öffnen" derselben Datei.
    let dir = edit_session_dir(s.session_id).unwrap();
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let existing = dir.join("grown.conf");
    tokio::fs::write(&existing, b"FRUEHERE-KOPIE")
        .await
        .unwrap();

    let (result, _) = open_for_editing(&s, s.normal_channel(), "/t/grown.conf", LIMIT).await;

    assert_too_large_for_editing(result);
    assert_eq!(
        normal.read_calls(),
        1,
        "gelesen wurde — abgelehnt wird danach, am tatsächlichen Inhalt"
    );
    assert_eq!(
        tokio::fs::read(&existing).await.unwrap(),
        b"FRUEHERE-KOPIE".to_vec(),
        "die frühere Kopie darf nicht überschrieben werden (A1.3)"
    );
    let _ = tokio::fs::remove_dir_all(&dir).await;
}

/// Spec 0086, T6: Absicherung gegen ein `>=` statt `>` — genau die Grenze
/// wird geöffnet. Beide Prüfungen einzeln: die per `stat` mit der **echten**
/// Konstante (`stat` meldet genau 50 MB, gelesen wird wenig), die nach dem
/// Lesen mit der parametrisierten Grenze.
#[tokio::test]
async fn test_t6_exactly_the_limit_is_still_opened_at_both_checks() {
    // (a) `stat` meldet genau 50 MB — die Vorabprüfung darf nicht greifen.
    let at_limit_by_stat = MismatchSftp::claiming(MAX_EDIT_OPEN_BYTES, 11);
    let s = setup(at_limit_by_stat, untouched()).await;
    let (result, dir) = open_for_editing(
        &s,
        s.normal_channel(),
        "/t/at-limit.conf",
        MAX_EDIT_OPEN_BYTES,
    )
    .await;
    result.expect("genau 50 MB laut `stat` liegen noch innerhalb der Grenze");
    let _ = tokio::fs::remove_dir_all(&dir).await;

    // (b) gelesen werden genau `LIMIT` Bytes — die Prüfung danach darf nicht
    // greifen.
    const LIMIT: u64 = 64;
    let at_limit_by_content = MismatchSftp::claiming(10, LIMIT as usize);
    let s = setup(at_limit_by_content, untouched()).await;
    let (result, dir) =
        open_for_editing(&s, s.normal_channel(), "/t/at-limit-read.conf", LIMIT).await;
    let dto = result.expect("genau die Grenze an gelesenen Bytes liegt noch darin");
    assert_eq!(
        tokio::fs::read(&dto.local_path).await.unwrap().len(),
        LIMIT as usize
    );
    let _ = tokio::fs::remove_dir_all(&dir).await;
}

// --- T7: A1.4, beide Grenzen gelten auch erhöht -----------------------------

/// Spec 0086, T7/A1.4: T1 über den **erhöhten** Kanal. Die Grenze sitzt im
/// gemeinsamen Befehlsrumpf, nicht am Kanal — der normale Kanal derselben
/// Sitzung wird dabei nie angefasst.
#[tokio::test]
async fn test_t7_the_clipboard_limit_also_holds_on_the_elevated_channel() {
    let normal = untouched();
    let elevated = MismatchSftp::claiming(100, (MAX_TEXT_PREVIEW_BYTES + 1) as usize);
    let s = setup(normal.clone(), elevated.clone()).await;

    let result =
        with_browser_channel(
            s.session.clone(),
            s.elevated_channel(),
            |session, channel| async move {
                read_text_impl(&session, &channel, "/root/grown.txt").await
            },
        )
        .await;

    assert_too_large_for_clipboard(result);
    assert_eq!(elevated.read_calls(), 1);
    assert!(
        normal.ops().is_empty(),
        "der normale Kanal darf dabei nie berührt werden, war: {:?}",
        normal.ops()
    );
}

/// Spec 0086, T7/A1.4: T4 über den **erhöhten** Kanal.
#[tokio::test]
async fn test_t7_the_editing_limit_also_holds_on_the_elevated_channel() {
    let normal = untouched();
    let elevated = MismatchSftp::claiming(MAX_EDIT_OPEN_BYTES + 1, 0);
    let s = setup(normal.clone(), elevated.clone()).await;

    let (result, dir) = open_for_editing(
        &s,
        s.elevated_channel(),
        "/root/huge.bin",
        MAX_EDIT_OPEN_BYTES,
    )
    .await;

    assert_too_large_for_editing(result);
    assert_eq!(
        elevated.read_calls(),
        0,
        "auch erhöht wird eine per `stat` erkennbar zu große Datei nicht übertragen"
    );
    assert!(!dir.exists(), "auch erhöht entsteht nichts lokal: {dir:?}");
    assert!(
        normal.ops().is_empty(),
        "der normale Kanal darf dabei nie berührt werden, war: {:?}",
        normal.ops()
    );
}
