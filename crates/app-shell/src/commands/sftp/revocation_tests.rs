//! Spec 0085, A1 (T1–T9): Ein Widerruf des erhöhten Modus beendet auch
//! Browser-Befehle, die schon laufen.
//!
//! Angriffsbild: Ein rekursives Löschen oder chmod holte den erhöhten Kanal
//! einmal zu Befehlsbeginn und lief dann durch. Schaltete der Nutzer den
//! erhöhten Modus währenddessen aus, entfernte die Sitzung oder aktivierte
//! sie für einen anderen Nutzer neu, liefen die restlichen Schreibvorgänge
//! weiter mit den alten Rechten — und das Ausschalten wartete, bis die ganze
//! Rekursion durch war.
//!
//! Alle Tests hier fahren den **echten** Befehlsrumpf (`delete_impl`,
//! `chmod_impl`, …) durch den **echten** gemeinsamen Rahmen
//! (`with_browser_channel`, dieselbe Funktion, die `run_browser_command` im
//! Produktivpfad benutzt) — nur das Auflösen von Sitzung und Kanal aus dem
//! `AppState` fehlt, weil ein Tauri-`State` im Unit-Test nicht baubar ist.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use tokio::sync::Notify;

use ssh_manager_core::ssh::mock::MockSftpSession;
use ssh_manager_core::ssh::{
    CommandOutput, InteractiveShell, PtySize, RemoteEntry, SftpSession, SshError, SshTransport,
};

use app_logic::session::{Session, SessionManager};
use app_logic::state::SessionId;

use super::*;
use crate::commands::elevation::{with_browser_channel, BrowserChannel};
use crate::elevated_sftp::{ElevatedSftp, ElevatedSftpRegistry, ElevatedSftpSlot};

/// Frist für alles, was in diesen Tests auf ein Signal wartet. Ein Test, der
/// hier hängen bliebe, soll sichtbar scheitern statt den Testlauf anzuhalten
/// (Vorgabe aus `CLAUDE.md`/Coder-Skill für den Gegenbeweis).
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

// --- Test-Double für den erhöhten Kanal --------------------------------------

/// Ein Eintrag des In-Memory-Baums hinter [`GatedSftp`].
#[derive(Clone)]
struct Entry {
    is_dir: bool,
    permissions: u32,
    content: Vec<u8>,
}

/// Der erhöhte Kanal als Test-Double: deterministischer Verzeichnisbaum,
/// Protokoll **jeder** Operation, und ein Haltepunkt nach der k-ten
/// Operation, an dem der Test widerrufen kann.
///
/// Deterministisch statt gegen `ssh_transport::LocalFileSession`: die
/// Reihenfolge von `read_dir` auf einem echten Dateisystem ist nicht
/// festgelegt, dann wäre „nach der k-ten Operation" keine reproduzierbare
/// Stelle im Ablauf.
#[derive(Clone)]
struct GatedSftp(Arc<Gate>);

struct Gate {
    entries: StdMutex<BTreeMap<String, Entry>>,
    ops: StdMutex<Vec<String>>,
    /// Nach der so vielten Operation hält der Mock an; `0` = nie.
    pause_after: usize,
    paused: AtomicBool,
    reached: Notify,
    released: AtomicBool,
    release: Notify,
    /// T5: Nach der Freigabe hält **jede** weitere Operation für immer an.
    /// Im behobenen Stand kommt keine weitere mehr an (der Widerruf greift
    /// vorher); in der kaputten Variante wartet das Deaktivieren hier
    /// sichtbar auf den Rest des Befehls und der Test scheitert mit Frist.
    stall_after_release: bool,
    stall: Notify,
}

impl GatedSftp {
    fn new(entries: &[(&str, bool)], pause_after: usize, stall_after_release: bool) -> Self {
        let entries = entries
            .iter()
            .map(|(path, is_dir)| {
                (
                    (*path).to_string(),
                    Entry {
                        is_dir: *is_dir,
                        permissions: if *is_dir { 0o755 } else { 0o644 },
                        content: b"ROOT-INHALT".to_vec(),
                    },
                )
            })
            .collect();
        Self(Arc::new(Gate {
            entries: StdMutex::new(entries),
            ops: StdMutex::new(Vec::new()),
            pause_after,
            paused: AtomicBool::new(false),
            reached: Notify::new(),
            released: AtomicBool::new(false),
            release: Notify::new(),
            stall_after_release,
            stall: Notify::new(),
        }))
    }

    /// Ein Baum mit zwei Dateien, einem Unterordner und einer Datei darin.
    fn small_tree(pause_after: usize) -> Self {
        Self::new(
            &[
                ("/t", true),
                ("/t/a.txt", false),
                ("/t/b.txt", false),
                ("/t/sub", true),
                ("/t/sub/c.txt", false),
            ],
            pause_after,
            false,
        )
    }

    /// Derselbe Aufbau, aber mit vielen Dateien — damit sichtbar wird, dass
    /// nach dem Widerruf wirklich **nichts** mehr läuft und das Deaktivieren
    /// nicht auf den Rest wartet (T5).
    fn large_tree(pause_after: usize, stall_after_release: bool) -> Self {
        let mut paths: Vec<String> = vec!["/t".to_string()];
        for i in 0..30 {
            paths.push(format!("/t/f{i:02}.txt"));
        }
        let entries: Vec<(&str, bool)> = paths
            .iter()
            .map(|p| (p.as_str(), p == "/t"))
            .collect::<Vec<_>>();
        Self::new(&entries, pause_after, stall_after_release)
    }

    fn ops(&self) -> Vec<String> {
        self.0.ops.lock().unwrap().clone()
    }

    fn op_count(&self) -> usize {
        self.0.ops.lock().unwrap().len()
    }

    fn exists(&self, path: &str) -> bool {
        self.0.entries.lock().unwrap().contains_key(path)
    }

    fn permissions(&self, path: &str) -> Option<u32> {
        self.0
            .entries
            .lock()
            .unwrap()
            .get(path)
            .map(|e| e.permissions)
    }

    /// Wartet, bis der Mock am Haltepunkt steht. `notified()` wird **vor**
    /// der Flag-Prüfung angelegt, sonst ginge ein Signal zwischen Prüfung
    /// und `await` verloren.
    async fn wait_until_paused(&self) {
        tokio::time::timeout(DEADLINE, async {
            loop {
                let notified = self.0.reached.notified();
                if self.0.paused.load(Ordering::SeqCst) {
                    return;
                }
                notified.await;
            }
        })
        .await
        .expect("der Mock muss den Haltepunkt erreichen");
    }

    fn release(&self) {
        self.0.released.store(true, Ordering::SeqCst);
        self.0.release.notify_waiters();
    }
}

impl Gate {
    async fn wait_for_release(&self) {
        loop {
            let notified = self.release.notified();
            if self.released.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }

    /// Hält für immer an — nur erreichbar, wenn nach dem Widerruf noch eine
    /// Operation durchkommt (s. [`Gate::stall_after_release`]).
    async fn stall_forever(&self) {
        loop {
            self.stall.notified().await;
        }
    }
}

impl GatedSftp {
    /// Protokolliert die Operation und bedient Haltepunkt bzw. Dauerhalt.
    async fn record(&self, op: String) {
        let index = {
            let mut ops = self.0.ops.lock().unwrap();
            ops.push(op);
            ops.len()
        };
        if self.0.stall_after_release && self.0.released.load(Ordering::SeqCst) {
            self.0.stall_forever().await;
        }
        if index == self.0.pause_after && !self.0.paused.swap(true, Ordering::SeqCst) {
            self.0.reached.notify_waiters();
            self.0.wait_for_release().await;
        }
    }

    fn entry(&self, path: &str) -> Result<RemoteEntry, SshError> {
        let entries = self.0.entries.lock().unwrap();
        let entry = entries
            .get(path)
            .ok_or_else(|| SshError::ChannelError(format!("Datei nicht gefunden: {path}")))?;
        Ok(remote_entry(path, entry))
    }
}

fn remote_entry(path: &str, entry: &Entry) -> RemoteEntry {
    RemoteEntry {
        name: path.rsplit('/').next().unwrap_or(path).to_string(),
        path: path.to_string(),
        is_dir: entry.is_dir,
        size: entry.content.len() as u64,
        permissions: entry.permissions,
        modified: None,
        uid: None,
        gid: None,
        owner: None,
        group: None,
    }
}

#[async_trait]
impl SftpSession for GatedSftp {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        self.record(format!("list_dir {path}")).await;
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let entries = self.0.entries.lock().unwrap();
        Ok(entries
            .iter()
            .filter(|(p, _)| {
                p.strip_prefix(prefix.as_str())
                    .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
            })
            .map(|(p, e)| remote_entry(p, e))
            .collect())
    }

    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.record(format!("read_file {path}")).await;
        let entries = self.0.entries.lock().unwrap();
        entries
            .get(path)
            .map(|e| e.content.clone())
            .ok_or_else(|| SshError::ChannelError(format!("Datei nicht gefunden: {path}")))
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError> {
        self.record(format!("write_file {path}")).await;
        self.0.entries.lock().unwrap().insert(
            path.to_string(),
            Entry {
                is_dir: false,
                permissions: 0o644,
                content: content.to_vec(),
            },
        );
        Ok(())
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.record(format!("stat {path}")).await;
        self.entry(path)
    }

    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.record(format!("lstat {path}")).await;
        self.entry(path)
    }

    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        self.record(format!("remove {path}")).await;
        let mut entries = self.0.entries.lock().unwrap();
        match entries.get(path) {
            None => Err(SshError::ChannelError(format!(
                "Datei nicht gefunden: {path}"
            ))),
            Some(entry) if entry.is_dir => Err(SshError::ChannelError(format!(
                "{path} ist ein Verzeichnis"
            ))),
            Some(_) => {
                entries.remove(path);
                Ok(())
            }
        }
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        self.record(format!("rename {from} -> {to}")).await;
        let mut entries = self.0.entries.lock().unwrap();
        let Some(entry) = entries.remove(from) else {
            return Err(SshError::ChannelError(format!(
                "Datei nicht gefunden: {from}"
            )));
        };
        entries.insert(to.to_string(), entry);
        Ok(())
    }

    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.record(format!("create_dir {path}")).await;
        self.0.entries.lock().unwrap().insert(
            path.to_string(),
            Entry {
                is_dir: true,
                permissions: 0o755,
                content: Vec::new(),
            },
        );
        Ok(())
    }

    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.record(format!("remove_dir {path}")).await;
        let mut entries = self.0.entries.lock().unwrap();
        let prefix = format!("{}/", path.trim_end_matches('/'));
        if entries.keys().any(|p| p.starts_with(prefix.as_str())) {
            // Wie SFTP `RMDIR`: nur ein leeres Verzeichnis lässt sich
            // entfernen. So merkt T9, wenn die Reihenfolge kippt.
            return Err(SshError::ChannelError(format!(
                "Verzeichnis nicht leer: {path}"
            )));
        }
        entries.remove(path);
        Ok(())
    }

    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError> {
        self.record(format!("set_permissions {path} {mode:o}"))
            .await;
        let mut entries = self.0.entries.lock().unwrap();
        match entries.get_mut(path) {
            None => Err(SshError::ChannelError(format!(
                "Datei nicht gefunden: {path}"
            ))),
            Some(entry) => {
                entry.permissions = mode;
                Ok(())
            }
        }
    }
}

/// Meldet sein eigenes Verworfenwerden — für T15 (Spec 0085, A2.2) und, hier,
/// als *neuer* Kanal in T4: er darf nie berührt werden.
#[derive(Clone)]
struct CountingSftp(Arc<StdMutex<Vec<String>>>);

#[async_trait]
impl SftpSession for CountingSftp {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        self.0.lock().unwrap().push(format!("list_dir {path}"));
        Ok(Vec::new())
    }
    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.0.lock().unwrap().push(format!("read_file {path}"));
        Ok(Vec::new())
    }
    async fn write_file(&mut self, path: &str, _content: &[u8]) -> Result<(), SshError> {
        self.0.lock().unwrap().push(format!("write_file {path}"));
        Ok(())
    }
    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.0.lock().unwrap().push(format!("stat {path}"));
        Err(SshError::ChannelError("kein Eintrag".to_string()))
    }
    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.0.lock().unwrap().push(format!("lstat {path}"));
        Err(SshError::ChannelError("kein Eintrag".to_string()))
    }
    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        self.0.lock().unwrap().push(format!("remove {path}"));
        Ok(())
    }
    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        self.0.lock().unwrap().push(format!("rename {from} {to}"));
        Ok(())
    }
    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.0.lock().unwrap().push(format!("create_dir {path}"));
        Ok(())
    }
    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.0.lock().unwrap().push(format!("remove_dir {path}"));
        Ok(())
    }
    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError> {
        self.0
            .lock()
            .unwrap()
            .push(format!("set_permissions {path} {mode:o}"));
        Ok(())
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

/// Sitzung, Zuordnung und beide Kanäle, wie die `sftp_*`-Befehle sie zur
/// Laufzeit vorfinden.
struct Setup {
    sessions: Arc<SessionManager>,
    registry: Arc<ElevatedSftpRegistry>,
    session_id: SessionId,
    session: Arc<Session>,
    /// Prüf-Handle auf den **normalen** Kanal der Sitzung (T8): er darf in
    /// keinem Widerrufs-Test eine Operation sehen.
    normal: MockSftpSession,
}

async fn setup(elevated: Box<dyn SftpSession>) -> Setup {
    let session = Arc::new(app_logic::test_support::session_with_transport(Box::new(
        NoTransport,
    )));
    let normal = MockSftpSession::new().with_file("/t/a.txt", b"USER-INHALT".to_vec());
    session.set_sftp_for_tests(Box::new(normal.clone())).await;

    let sessions = Arc::new(SessionManager::new());
    let session_id = SessionId::new_v4();
    sessions.insert(session_id, session.clone());

    let registry = Arc::new(ElevatedSftpRegistry::default());
    registry.insert_for_tests(
        session_id,
        ElevatedSftp {
            target_user: "root".to_string(),
            sftp: elevated,
        },
    );

    Setup {
        sessions,
        registry,
        session_id,
        session,
        normal,
    }
}

impl Setup {
    fn elevated_channel(&self) -> BrowserChannel {
        BrowserChannel::for_tests(&self.registry, self.session_id, Some("root".to_string()))
    }

    fn normal_channel(&self) -> BrowserChannel {
        BrowserChannel::for_tests(&self.registry, self.session_id, None)
    }

    fn slot(&self) -> ElevatedSftpSlot {
        self.registry
            .slot(self.session_id, &crate::test_support::elevation::access())
            .expect("Vorbedingung: erhöhter Kanal aktiv")
    }

    /// Wartet, bis der Widerruf wirkt — er ist der sofort wirksame Teil und
    /// geht dem Herausnehmen des Kanalwerts voraus (Spec 0084, §9).
    async fn wait_until_revoked(&self, slot: &ElevatedSftpSlot) {
        tokio::time::timeout(DEADLINE, async {
            while !slot.is_revoked() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("der Widerruf muss sofort wirken, nicht erst nach dem Befehl");
    }
}

/// Fährt einen Befehlsrumpf durch **denselben** gemeinsamen Rahmen wie der
/// Produktivpfad (`run_browser_command` ruft dieselbe Funktion) und gibt den
/// laufenden Task zurück, damit der Test währenddessen widerrufen kann.
fn spawn_command<T, F, Fut>(
    session: Arc<Session>,
    channel: BrowserChannel,
    body: F,
) -> tokio::task::JoinHandle<CommandResult<T>>
where
    T: Send + 'static,
    F: FnOnce(Arc<Session>, BrowserChannel) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = CommandResult<T>> + Send,
{
    tokio::spawn(async move { with_browser_channel(session, channel, body).await })
}

async fn join<T>(handle: tokio::task::JoinHandle<CommandResult<T>>) -> CommandResult<T> {
    tokio::time::timeout(DEADLINE, handle)
        .await
        .expect("der Befehl muss enden, statt auf den widerrufenen Kanal zu warten")
        .expect("der Befehls-Task darf nicht panisch enden")
}

/// Spec 0085, A1.2: **gleich**, nicht „enthält". Ein Abbruch wegen Widerrufs
/// darf nicht als SFTP-Fehler oder Channel-Fehler beim Aufrufer ankommen.
fn assert_inactive<T: std::fmt::Debug>(result: CommandResult<T>) {
    let err = result.expect_err("nach dem Widerruf darf der Befehl nicht gelingen");
    assert_eq!(
        err.message,
        "Der erhöhte Modus ist nicht mehr aktiv (Verbindung getrennt oder ausgeschaltet) — \
         die Aktion wurde nicht ausgeführt. Bitte den erhöhten Modus erneut einschalten.",
        "der Abbruchgrund muss wörtlich ELEVATED_CHANNEL_INACTIVE sein"
    );
}

// --- Audit-Mitschnitt (T7) ---------------------------------------------------

/// Eingefangene `tracing`-Ereignisse, Feldname → Wert.
#[derive(Clone, Default)]
struct CapturedLogs(Arc<StdMutex<Vec<HashMap<String, String>>>>);

impl CapturedLogs {
    /// Die Audit-Zeilen zu `action` auf `path` (s.
    /// `elevation::audit_elevated_change`).
    fn audit_lines(&self, action: &str, path: &str) -> Vec<HashMap<String, String>> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|fields| {
                fields.get("action").map(String::as_str) == Some(action)
                    && fields.get("path").map(String::as_str) == Some(path)
                    && fields.get("elevated").map(String::as_str) == Some("true")
            })
            .cloned()
            .collect()
    }
}

/// Minimaler `tracing::Subscriber`, der nur Ereignisfelder sammelt.
///
/// Von Hand statt über `tracing-subscriber`: diese Kiste hat die Abhängigkeit
/// nicht, und für „welche Felder hatte das Ereignis" braucht es nichts
/// weiter. Als **Thread-Default** gesetzt (`tracing::subscriber::
/// set_default`), nicht global — `#[tokio::test]` fährt eine
/// Ein-Thread-Laufzeit, die auch die per `tokio::spawn` gestarteten Tasks auf
/// genau diesem Thread abarbeitet, der Mitschnitt erfasst sie also mit.
struct CapturingSubscriber(CapturedLogs);

impl tracing::Subscriber for CapturingSubscriber {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = HashMap::new();
        event.record(&mut FieldCollector(&mut fields));
        self.0 .0.lock().unwrap().push(fields);
    }
    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

struct FieldCollector<'a>(&'a mut HashMap<String, String>);

impl tracing::field::Visit for FieldCollector<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}

fn capture_logs() -> (CapturedLogs, tracing::subscriber::DefaultGuard) {
    let logs = CapturedLogs::default();
    let guard = tracing::subscriber::set_default(CapturingSubscriber(logs.clone()));
    (logs, guard)
}

// --- T1–T5: Widerruf mitten in einer Rekursion ------------------------------

/// Spec 0085, T1: Rekursives Löschen über den erhöhten Kanal, der Mock hält
/// nach der 5. Operation an (die erste Datei ist da schon gelöscht). Genau
/// dort schaltet der Nutzer die erhöhten Rechte aus.
///
/// Scheitert gegen den Stand vor diesem Schritt (Widerruf nur bei
/// Befehlsbeginn geprüft): dann laufen die restlichen `remove`/`remove_dir`
/// über den widerrufenen Kanal weiter und der Befehl endet mit `Ok`.
#[tokio::test]
async fn test_t1_disabling_during_a_recursive_delete_stops_it_immediately() {
    let elevated = GatedSftp::small_tree(5);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let command = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    );
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();
    assert!(
        elevated.ops().iter().any(|op| op.starts_with("remove /t/")),
        "Vorbedingung: mindestens ein Eintrag war schon gelöscht, war: {:?}",
        elevated.ops()
    );

    // Ausschalten: der Merker wirkt sofort, das Herausnehmen des Kanalwerts
    // wartet auf die gerade laufende Operation — deshalb in einem eigenen
    // Task.
    let (sessions, registry) = (s.sessions.clone(), s.registry.clone());
    let session_id = s.session_id;
    let disable = tokio::spawn(async move {
        crate::elevated_sftp::disable(
            &sessions,
            &registry,
            session_id,
            &crate::test_support::elevation::access(),
        )
        .await
    });
    s.wait_until_revoked(&slot).await;
    elevated.release();

    assert_inactive(join(command).await);
    tokio::time::timeout(DEADLINE, disable)
        .await
        .expect("das Ausschalten muss enden")
        .expect("der Ausschalt-Task darf nicht panisch enden")
        .expect("Ausschalten gelingt");

    assert_eq!(
        elevated.op_count(),
        ops_before,
        "nach dem Widerruf darf keine Operation mehr über den erhöhten Kanal laufen, war: {:?}",
        elevated.ops()
    );
    assert!(
        elevated.exists("/t") && elevated.exists("/t/sub/c.txt"),
        "der Rest des Baums bleibt stehen — der Befehl wurde abgebrochen"
    );
    // A1.6: kein Rückfall auf den normalen Kanal (T8).
    assert!(
        s.normal.calls().is_empty(),
        "der normale Kanal darf keine Operation dieses Befehls sehen, war: {:?}",
        s.normal.calls()
    );
}

/// Spec 0085, T2: dasselbe für rekursives chmod. Der Mock hält nach der 5.
/// Operation an — da hat der erste Eintrag die neuen Rechte schon.
#[tokio::test]
async fn test_t2_disabling_during_a_recursive_chmod_stops_it_immediately() {
    let elevated = GatedSftp::small_tree(5);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let command = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { chmod_impl(&session, &channel, "/t", 0o700, true).await },
    );
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();
    assert_eq!(
        elevated.permissions("/t/sub/c.txt"),
        Some(0o700),
        "Vorbedingung: mindestens ein Eintrag hatte die neuen Rechte schon"
    );

    let (sessions, registry) = (s.sessions.clone(), s.registry.clone());
    let session_id = s.session_id;
    let disable = tokio::spawn(async move {
        crate::elevated_sftp::disable(
            &sessions,
            &registry,
            session_id,
            &crate::test_support::elevation::access(),
        )
        .await
    });
    s.wait_until_revoked(&slot).await;
    elevated.release();

    assert_inactive(join(command).await);
    tokio::time::timeout(DEADLINE, disable)
        .await
        .expect("das Ausschalten muss enden")
        .expect("der Ausschalt-Task darf nicht panisch enden")
        .expect("Ausschalten gelingt");

    assert_eq!(
        elevated.op_count(),
        ops_before,
        "nach dem Widerruf darf kein weiteres set_permissions laufen, war: {:?}",
        elevated.ops()
    );
    assert_eq!(
        elevated.permissions("/t"),
        Some(0o755),
        "die Wurzel behält ihre alten Rechte — sie kam nach dem Widerruf dran"
    );
    assert!(s.normal.calls().is_empty());
}

/// Spec 0085, T3: wie T1, aber die Sitzung wird entfernt (Spec 0084, A2.1)
/// statt ausgeschaltet. Das Entfernen kehrt **sofort** zurück, ohne auf den
/// laufenden Befehl zu warten — es steht im `disconnect`-Pfad.
#[tokio::test]
async fn test_t3_removing_the_session_during_a_recursive_delete_stops_it_immediately() {
    let elevated = GatedSftp::small_tree(5);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let command = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    );
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();

    // Direkt, nicht in einem Task: das Entfernen darf gar nicht warten.
    let removed = s.registry.remove_session(&s.sessions, s.session_id);
    assert!(removed.is_some(), "die Sitzung war eingetragen");
    assert!(
        slot.is_revoked(),
        "A2.1: das Entfernen widerruft sofort, während der Befehl die Sperre noch hält"
    );

    elevated.release();
    assert_inactive(join(command).await);
    assert_eq!(
        elevated.op_count(),
        ops_before,
        "nach dem Trennen darf keine Operation mehr laufen, war: {:?}",
        elevated.ops()
    );
    assert!(s.normal.calls().is_empty());
}

/// Spec 0085, T4: wie T1, aber der Nutzer aktiviert den erhöhten Modus
/// mitten im Befehl für einen **anderen** Nutzer neu. Danach läuft keine
/// Operation dieses Befehls mehr — weder über den alten noch über den neuen
/// Kanal.
#[tokio::test]
async fn test_t4_reactivating_for_another_user_during_a_recursive_delete_stops_it() {
    let elevated = GatedSftp::small_tree(5);
    let new_channel_calls = Arc::new(StdMutex::new(Vec::new()));

    // Aufbau über den echten Aktivierungsweg, damit das Neu-Aktivieren
    // wirklich `insert_if_session_alive`/`revoke` durchläuft: der Transport
    // gibt erst den überwachten, dann den neuen Kanal heraus.
    let f = crate::test_support::elevation::fixture_with_channels(vec![
        Box::new(elevated.clone()),
        Box::new(CountingSftp(new_channel_calls.clone())),
    ]);
    let normal = MockSftpSession::new();
    f.session.set_sftp_for_tests(Box::new(normal.clone())).await;
    crate::elevated_sftp::enable(
        &f.ctx(),
        "deploy",
        None,
        None,
        &crate::test_support::elevation::access(),
    )
    .await
    .expect("Vorbedingung: Aktivieren als root gelingt");
    let slot = f
        .registry
        .slot(f.session_id, &crate::test_support::elevation::access())
        .expect("Vorbedingung: erhöhter Kanal aktiv");

    let channel = BrowserChannel::for_tests(&f.registry, f.session_id, Some("root".to_string()));
    let command = spawn_command(f.session.clone(), channel, |session, channel| async move {
        delete_impl(&session, &channel, "/t").await
    });
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();

    let f = Arc::new(f);
    let f_for_switch = f.clone();
    let switch = tokio::spawn(async move {
        crate::elevated_sftp::enable(
            &f_for_switch.ctx(),
            "deploy",
            None,
            Some("www-data"),
            &crate::test_support::elevation::access(),
        )
        .await
    });
    tokio::time::timeout(DEADLINE, async {
        while !slot.is_revoked() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("das Neu-Aktivieren widerruft den alten Kanal sofort");
    elevated.release();

    assert_inactive(join(command).await);
    tokio::time::timeout(DEADLINE, switch)
        .await
        .expect("das Umschalten muss enden")
        .expect("der Umschalt-Task darf nicht panisch enden")
        .expect("das Umschalten selbst gelingt");

    assert_eq!(
        elevated.op_count(),
        ops_before,
        "keine Operation mehr über den ALTEN Kanal, war: {:?}",
        elevated.ops()
    );
    assert!(
        new_channel_calls.lock().unwrap().is_empty(),
        "und keine über den NEUEN Kanal, war: {:?}",
        new_channel_calls.lock().unwrap()
    );
    assert!(normal.calls().is_empty(), "und keine über den normalen");
}

/// Spec 0085, T5: Das Ausschalten wartet höchstens auf die gerade laufende
/// einzelne SFTP-Operation, nicht auf den Rest des Befehls (A1.3).
///
/// Der Mock hält nach der 5. Operation an und lässt danach **jede** weitere
/// Operation für immer hängen. Im behobenen Stand kommt keine weitere mehr
/// an: der Befehl bricht ab, die Sperre wird frei, das Ausschalten kehrt
/// zurück. Gegen den Stand von vorher (Merker nur bei Befehlsbeginn, Sperre
/// bis zum Ende gehalten) läuft der Befehl in den Dauerhalt und das
/// Ausschalten scheitert mit Frist.
#[tokio::test]
async fn test_t5_disabling_waits_only_for_the_running_operation_not_for_the_rest() {
    let elevated = GatedSftp::large_tree(5, true);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let command = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    );
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();

    let (sessions, registry) = (s.sessions.clone(), s.registry.clone());
    let session_id = s.session_id;
    let disable = tokio::spawn(async move {
        crate::elevated_sftp::disable(
            &sessions,
            &registry,
            session_id,
            &crate::test_support::elevation::access(),
        )
        .await
    });
    s.wait_until_revoked(&slot).await;
    elevated.release();

    tokio::time::timeout(DEADLINE, disable)
        .await
        .expect("das Ausschalten darf nicht auf den Rest des Befehls warten")
        .expect("der Ausschalt-Task darf nicht panisch enden")
        .expect("Ausschalten gelingt");

    assert_inactive(join(command).await);
    assert_eq!(
        elevated.op_count(),
        ops_before,
        "Zähler der nach dem Widerruf ausgeführten Operationen muss 0 sein, war: {:?}",
        elevated.ops()
    );
    assert!(
        elevated.exists("/t/f29.txt"),
        "der große Rest des Baums ist unberührt"
    );
}

// --- T6/T6b/T6c: Befehle, die den Fehler selbst abfangen oder nur lesen -----

/// Spec 0085, T6: Die Lösch-Vorschau bricht ebenso ab — kein weiterer
/// Lesezugriff nach dem Widerruf, und ein Fehler statt eines Zählergebnisses.
#[tokio::test]
async fn test_t6_delete_preview_stops_at_a_revocation_instead_of_returning_counts() {
    let elevated = GatedSftp::small_tree(2);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let command = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_preview_impl(&session, &channel, "/t").await },
    );
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();

    s.registry.remove_session(&s.sessions, s.session_id);
    s.wait_until_revoked(&slot).await;
    elevated.release();

    assert_inactive(join(command).await);
    assert_eq!(
        elevated.op_count(),
        ops_before,
        "kein weiterer Lesezugriff nach dem Widerruf, war: {:?}",
        elevated.ops()
    );
}

/// Spec 0085, T6b: `sftp_read_text` und `sftp_open_for_editing` führen zwei
/// Operationen aus (`stat`, dann `read_file`). Ein Widerruf dazwischen lässt
/// die Datei ungelesen.
///
/// Scheitert gegen eine Lösung, die nur die Rekursionen absichert: dort
/// prüfte der zweite Zugriff nichts und der Dateiinhalt ginge über den
/// widerrufenen Kanal noch heraus.
#[tokio::test]
async fn test_t6b_a_revocation_between_stat_and_read_leaves_the_file_unread() {
    for command_name in ["read_text", "open_for_editing"] {
        let elevated = GatedSftp::small_tree(1);
        let s = setup(Box::new(elevated.clone())).await;
        let slot = s.slot();
        let session_id = s.session_id;

        let command = spawn_command(
            s.session.clone(),
            s.elevated_channel(),
            move |session, channel| async move {
                if command_name == "read_text" {
                    read_text_impl(&session, &channel, "/t/a.txt")
                        .await
                        .map(|_| ())
                } else {
                    open_for_editing_impl(&session, &channel, session_id, "/t/a.txt")
                        .await
                        .map(|_| ())
                }
            },
        );
        elevated.wait_until_paused().await;
        assert_eq!(
            elevated.ops(),
            vec!["stat /t/a.txt".to_string()],
            "{command_name}: Vorbedingung — der Mock steht direkt nach dem stat"
        );

        s.registry.remove_session(&s.sessions, session_id);
        s.wait_until_revoked(&slot).await;
        elevated.release();

        assert_inactive(join(command).await);
        assert!(
            !elevated.ops().iter().any(|op| op.starts_with("read_file")),
            "{command_name}: die Datei darf nach dem Widerruf nicht gelesen werden, war: {:?}",
            elevated.ops()
        );
        assert!(s.normal.calls().is_empty(), "{command_name}: kein Rückfall");
    }
}

/// Spec 0085, T6c: `sftp_exists` beantwortet „gibt es den Pfad?" mit
/// `stat(..).is_ok()` und verschluckt damit jeden Fehler — auch einen
/// Widerruf. Der Abbruch-Vermerk des Befehls und die zentrale Übersetzung
/// machen daraus trotzdem `ELEVATED_CHANNEL_INACTIVE` statt `Ok(false)`.
///
/// Der Widerruf sitzt hier **vor** dem `stat`, zwischen Zugang und Operation
/// — dafür der Haltepunkt am Slot (`set_before_operation_hook`): `sftp_exists`
/// hat nur diese eine Operation, es gibt also keinen zweiten Zugriff, an dem
/// die Verschränkung von selbst entstünde.
///
/// Scheitert gegen eine Lösung mit Prüfung je Operation, aber **ohne**
/// zentrale Übersetzung: dann kommt beim Aufrufer `Ok(false)` an — die
/// Aussage „der Pfad existiert nicht", obwohl niemand nachgesehen hat.
#[tokio::test]
async fn test_t6c_exists_reports_the_revocation_instead_of_a_plain_false() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let sessions = s.sessions.clone();
    let registry = s.registry.clone();
    let session_id = s.session_id;
    slot.set_before_operation_hook(Box::new(move || {
        registry.remove_session(&sessions, session_id);
    }));

    let result = with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { exists_impl(&session, &channel, "/t/a.txt").await },
    )
    .await;

    assert!(
        slot.is_revoked(),
        "Vorbedingung: der Haltepunkt hat vor der Operation widerrufen"
    );
    assert_inactive(result);
    assert!(
        elevated.ops().is_empty(),
        "das stat darf nach dem Widerruf nicht mehr laufen, war: {:?}",
        elevated.ops()
    );
}

/// spec-reviewer-Fund (Runde 1): Der Zugang zum erhöhten Kanal prüft den
/// Widerruf **zweimal** — vor dem Warten auf die Kanal-Sperre und noch einmal,
/// nachdem er sie bekommen hat. Die zweite Prüfung ist die, die zählt, wenn
/// ein Zugriff die erste schon passiert hat und dann wartet: `tokio::sync::
/// Mutex` ist fair, ein bereits wartender Zugriff kommt also **vor** dem
/// Widerruf an die Reihe. Durch die Sperre je Operation (A1.3) ist dieses
/// Fenster häufiger als vorher.
///
/// Der Haltepunkt am Slot sitzt genau darin. Der Widerruf läuft hier bewusst
/// über `remove_session`: das setzt nur den Merker und lässt den Kanalwert
/// stehen. Nur so hängt der Abbruch wirklich an der zweiten Prüfung — wäre
/// der Wert herausgenommen, scheiterte der Zugriff schon daran.
///
/// Gegenbeweis (belegt): Ohne die zweite Prüfung läuft die Operation über den
/// widerrufenen Kanal, und der ganze Baum wird gelöscht.
#[tokio::test]
async fn test_a_revocation_while_an_access_waits_for_the_lock_still_stops_it() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let sessions = s.sessions.clone();
    let registry = s.registry.clone();
    let session_id = s.session_id;
    slot.set_before_operation_hook(Box::new(move || {
        registry.remove_session(&sessions, session_id);
    }));

    let result = with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    )
    .await;

    assert!(
        slot.is_revoked(),
        "Vorbedingung: der Haltepunkt hat nach der Vorprüfung widerrufen"
    );
    assert_inactive(result);
    assert!(
        elevated.ops().is_empty(),
        "keine einzige Operation darf über den widerrufenen Kanal laufen, war: {:?}",
        elevated.ops()
    );
    assert!(
        elevated.exists("/t") && elevated.exists("/t/sub/c.txt"),
        "und der Baum ist unberührt"
    );
    assert!(s.normal.calls().is_empty(), "kein Rückfall");
}

// --- T7: Audit --------------------------------------------------------------

/// Spec 0085, T7 (A1.4): Ein abgebrochener erhöhter `delete`/`chmod`, bei dem
/// mindestens eine Operation lief, hinterlässt **genau eine** Audit-Zeile
/// `ok = false` mit dem Ziel-Nutzer, unter dem die schon ausgeführten
/// Operationen liefen.
///
/// Scheitert gegen die Variante, die den Ziel-Nutzer erst nach dem Widerruf
/// abfragt: dann meldet der Kanal keinen Nutzer mehr und die Zeile fehlt
/// ganz — die erhöhte Änderung wäre nicht protokolliert.
#[tokio::test]
async fn test_t7_an_aborted_elevated_change_is_audited_once_with_its_target_user() {
    for action in ["delete", "chmod"] {
        let elevated = GatedSftp::small_tree(5);
        let s = setup(Box::new(elevated.clone())).await;
        let slot = s.slot();
        let (logs, _capture) = capture_logs();

        let command = spawn_command(
            s.session.clone(),
            s.elevated_channel(),
            move |session, channel| async move {
                if action == "delete" {
                    delete_impl(&session, &channel, "/t").await
                } else {
                    chmod_impl(&session, &channel, "/t", 0o700, true)
                        .await
                        .map(|_| ())
                }
            },
        );
        elevated.wait_until_paused().await;
        s.registry.remove_session(&s.sessions, s.session_id);
        s.wait_until_revoked(&slot).await;
        elevated.release();
        assert_inactive(join(command).await);

        let lines = logs.audit_lines(action, "/t");
        assert_eq!(
            lines.len(),
            1,
            "{action}: genau eine Audit-Zeile erwartet, war: {lines:?}"
        );
        assert_eq!(
            lines[0].get("ok").map(String::as_str),
            Some("false"),
            "{action}: die Zeile darf keinen Erfolg behaupten"
        );
        assert_eq!(
            lines[0].get("target_user").map(String::as_str),
            Some("root"),
            "{action}: mit dem Ziel-Nutzer, unter dem die Operationen liefen"
        );
    }
}

/// Spec 0085, T7, zweiter Teil: Scheitert schon der **Zugriff** auf den
/// Kanal, gibt es wie bisher keine Zeile — es lief keine Operation, es ist
/// nichts zu protokollieren.
#[tokio::test]
async fn test_t7_a_revocation_before_the_first_access_writes_no_audit_line() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let (logs, _capture) = capture_logs();

    s.registry.remove_session(&s.sessions, s.session_id);

    let result = with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    )
    .await;

    assert_inactive(result);
    assert!(
        logs.audit_lines("delete", "/t").is_empty(),
        "ohne ausgeführte Operation keine Audit-Zeile, war: {:?}",
        logs.audit_lines("delete", "/t")
    );
    assert!(elevated.ops().is_empty());
}

// --- T9: Regression ---------------------------------------------------------

/// Spec 0085, T9: Ohne Widerruf verhalten sich rekursives Löschen und chmod
/// wie bisher — über den nicht widerrufenen erhöhten Kanal bis `Ok`.
#[tokio::test]
async fn test_t9_without_a_revocation_recursive_delete_and_chmod_still_succeed() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;

    let changed = with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { chmod_impl(&session, &channel, "/t", 0o700, true).await },
    )
    .await
    .expect("ohne Widerruf muss das rekursive chmod gelingen");
    assert_eq!(changed, 5, "Wurzel, zwei Dateien, Unterordner, Datei darin");
    assert_eq!(elevated.permissions("/t"), Some(0o700));
    assert_eq!(elevated.permissions("/t/sub/c.txt"), Some(0o700));

    with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    )
    .await
    .expect("ohne Widerruf muss das rekursive Löschen gelingen");
    assert!(
        !elevated.exists("/t") && !elevated.exists("/t/sub/c.txt"),
        "der ganze Baum ist weg, war: {:?}",
        elevated.ops()
    );
    assert!(s.normal.calls().is_empty());
}

/// Spec 0085, T9/A1.5: Über den **normalen** Kanal ändert sich nichts — er
/// wird wie bisher für die Dauer des Befehls gesperrt, kennt keinen Widerruf,
/// und der erhöhte Kanal bleibt dabei unberührt.
#[tokio::test]
async fn test_t9_the_normal_channel_is_unaffected_by_the_revocation_machinery() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;

    // Der erhöhte Kanal ist aktiv — und wird trotzdem nicht berührt.
    let text = with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move { read_text_impl(&session, &channel, "/t/a.txt").await },
    )
    .await
    .expect("über den normalen Kanal muss das Lesen gelingen");

    assert_eq!(text, "USER-INHALT");
    assert!(
        elevated.ops().is_empty(),
        "der erhöhte Kanal darf dabei nie berührt werden, war: {:?}",
        elevated.ops()
    );

    // Und ein Widerruf des erhöhten Kanals lässt den normalen Befehl
    // unbeeindruckt.
    s.registry.remove_session(&s.sessions, s.session_id);
    let text = with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move { read_text_impl(&session, &channel, "/t/a.txt").await },
    )
    .await
    .expect("der Widerruf des erhöhten Kanals betrifft den normalen nicht");
    assert_eq!(text, "USER-INHALT");
}

/// Spec 0085, T9 (Ergänzung nach dem spec-reviewer-Fund aus Runde 1):
/// **rekursives** Löschen und chmod über den normalen Kanal, und zwar durch
/// denselben neuen Wrapper-Pfad (`BrowserSftp::Normal`), den die Befehle zur
/// Laufzeit nehmen.
///
/// Die bestehenden `sftp_mutation_tests` rufen `delete_recursive`/
/// `chmod_recursive` direkt mit einem `&mut dyn SftpSession` auf und gehen am
/// Wrapper vorbei — für den normalen Kanal war er damit nirgends von Befehl
/// bis Kanal abgedeckt.
#[tokio::test]
async fn test_t9_recursive_delete_and_chmod_over_the_normal_channel_behave_as_before() {
    let normal_tree = GatedSftp::small_tree(0);
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    // Der normale Kanal bekommt denselben Baum wie der erhöhte — so ist am
    // Protokoll ablesbar, dass wirklich der normale benutzt wurde.
    s.session
        .set_sftp_for_tests(Box::new(normal_tree.clone()))
        .await;

    let changed = with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move { chmod_impl(&session, &channel, "/t", 0o700, true).await },
    )
    .await
    .expect("rekursives chmod über den normalen Kanal muss gelingen");
    assert_eq!(changed, 5);
    assert_eq!(normal_tree.permissions("/t"), Some(0o700));
    assert_eq!(normal_tree.permissions("/t/sub/c.txt"), Some(0o700));

    with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    )
    .await
    .expect("rekursives Löschen über den normalen Kanal muss gelingen");
    assert!(
        !normal_tree.exists("/t") && !normal_tree.exists("/t/sub/c.txt"),
        "der ganze Baum ist weg, war: {:?}",
        normal_tree.ops()
    );

    assert!(
        elevated.ops().is_empty(),
        "der erhöhte Kanal darf dabei nie berührt werden, war: {:?}",
        elevated.ops()
    );
}
