//! Laufende Server-Sessions (Spec 0007, Abschnitt 3) und deren
//! Nebenläufigkeits-Modell.
//!
//! **Drei verschiedene Synchronisierungsmechanismen, bewusst nicht
//! einheitlich `Mutex<HashMap<...>>` überall:**
//!
//! 1. `SessionManager`s äußere Map (`SessionId -> Arc<Session>`) ist ein
//!    simples `std::sync::Mutex`: sie wird nur für kurze, nie über einen
//!    `.await`-Punkt hinweg gehaltene Zeigervorgänge (Einfügen/Nachschlagen/
//!    Entfernen) gesperrt — dafür ist eine synchrone Std-Mutex leichter und
//!    genügt völlig; `tokio::sync::Mutex` wäre hier nur unnötiger Overhead.
//! 2. `Session.transport` (seit Spec 0086 gekapselt in [`SessionTransport`])
//!    und `Session.context` nutzen dagegen
//!    `tokio::sync::Mutex`: beide werden über `.await`-Punkte hinweg
//!    gehalten (`SshTransport::execute()`, `AiProvider::send()`-Stream
//!    konsumieren) — eine `std::sync::MutexGuard` über einen Await-Punkt
//!    hinweg zu halten ist nicht `Send` und lässt sich in einer async
//!    Tauri-Command-Funktion nicht compilieren.
//! 3. Der interaktive Terminal-Kanal (`InteractiveShell`) bekommt **keinen**
//!    Mutex, sondern einen Aktor: ein einzelner Hintergrund-Task besitzt
//!    die `Box<dyn InteractiveShell>` exklusiv und wählt per `tokio::select!`
//!    zwischen "nächster Chunk vom Server" (`shell.read()`, kann beliebig
//!    lange blockieren) und "nächstes Kommando vom Frontend"
//!    (`terminal_input`/`terminal_resize` über einen `mpsc`-Kanal). Ein
//!    Mutex um die Shell hätte hier ein echtes Problem: `read()` "blockiert,
//!    bis Daten verfügbar sind" (s. `core::ssh::InteractiveShell`-Doc) — ein
//!    Hintergrund-Task, der den Mutex während eines laufenden `read()`-Awaits
//!    hält, würde `terminal_input`/`terminal_resize` bis zum nächsten
//!    Server-Chunk blockieren und die Eingabe spürbar verzögern. Der Aktor
//!    umgeht das vollständig, da nur er selbst je auf die Shell zugreift.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use ssh_manager_core::ai::{
    AiProvider, ChatMessage, MessageContent, OutputRedactor, SessionContext,
};
use ssh_manager_core::filter::{EvalContext, EvaluationTrace, FilterEngine, PolicyStore};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{PtySize, SftpSession, SshError, SshTransport};

use crate::confirmation::ConfirmationRegistry;
use crate::events::{emit_connection_status_changed, ConnectionStatus, EventEmitter};
use crate::state::{ActionId, SessionId};

/// Issue #355: the stored sudo password of a session. Behind a lock so a
/// reconnect can replace it together with the redactor.
pub struct SwappableSudoPassword(std::sync::RwLock<Option<Arc<secrecy::SecretString>>>);

impl SwappableSudoPassword {
    pub fn new(value: Option<secrecy::SecretString>) -> Self {
        Self(std::sync::RwLock::new(value.map(Arc::new)))
    }

    pub fn get(&self) -> Option<Arc<secrecy::SecretString>> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn is_some(&self) -> bool {
        self.get().is_some()
    }

    pub fn is_none(&self) -> bool {
        self.get().is_none()
    }

    fn replace(&self, value: Option<secrecy::SecretString>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = value.map(Arc::new);
    }
}

/// Issue #355: the redactor of a session. Behind a lock so a reconnect can
/// rebuild it with the sudo password read again from the keychain; every
/// reader sees either the old or the new redactor, never none.
pub struct SwappableRedactor {
    inner: std::sync::RwLock<Arc<dyn OutputRedactor>>,
    /// Issue #464: literal secret values inserted by an outgoing transform.
    /// They survive a reconnect (the swap only replaces `inner`) and are
    /// masked before the pattern-based redactor runs.
    literal_secrets: std::sync::RwLock<Vec<Vec<u8>>>,
}

impl SwappableRedactor {
    pub fn new(redactor: Box<dyn OutputRedactor>) -> Self {
        Self {
            inner: std::sync::RwLock::new(Arc::from(redactor)),
            literal_secrets: std::sync::RwLock::new(Vec::new()),
        }
    }

    fn current(&self) -> Arc<dyn OutputRedactor> {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn replace(&self, redactor: Box<dyn OutputRedactor>) {
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = Arc::from(redactor);
    }

    /// Issue #635: replaces the pattern-based redactor **and** the literal
    /// secret set with exactly `secrets` (empty values ignored). Only the
    /// extension rebuild path calls this; every other swap (reconnect) keeps
    /// the literal set add-only. Text stored already redacted is not touched.
    fn replace_with_literals(
        &self,
        redactor: Box<dyn OutputRedactor>,
        secrets: &[secrecy::SecretString],
    ) {
        use secrecy::ExposeSecret;
        // Build the complete new set first, then install it with a single
        // assignment under one write guard: a concurrent reader sees either
        // the old or the new set, never an empty one (no fail-open window).
        let mut fresh: Vec<Vec<u8>> = Vec::new();
        for secret in secrets {
            let value = secret.expose_secret().as_bytes();
            if !value.is_empty() && !fresh.iter().any(|k| k == value) {
                fresh.push(value.to_vec());
            }
        }
        // Longest first, so a secret that contains another is masked whole.
        fresh.sort_by_key(|k| std::cmp::Reverse(k.len()));
        let mut known = self
            .literal_secrets
            .write()
            .unwrap_or_else(|e| e.into_inner());
        *known = fresh;
        drop(known);
        self.replace(redactor);
    }

    /// Issue #464: from now on `secrets` are masked wherever this redactor
    /// runs (live output, result, ledger, history, logs). Called before the
    /// command that uses them starts, so its own output is already covered.
    /// Empty values are ignored (they would match everywhere).
    pub fn add_secret_values(&self, secrets: &[secrecy::SecretString]) {
        use secrecy::ExposeSecret;
        let mut known = self
            .literal_secrets
            .write()
            .unwrap_or_else(|e| e.into_inner());
        for secret in secrets {
            let value = secret.expose_secret().as_bytes();
            if !value.is_empty() && !known.iter().any(|k| k == value) {
                known.push(value.to_vec());
            }
        }
        // Longest first, so a secret that contains another is masked whole.
        known.sort_by_key(|k| std::cmp::Reverse(k.len()));
    }

    fn mask_literals(&self, bytes: &[u8]) -> Vec<u8> {
        let known = self
            .literal_secrets
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let placeholder = ssh_manager_core::ai::REDACTED_PLACEHOLDER.as_bytes();
        let mut out = bytes.to_vec();
        for secret in known.iter() {
            let mut next = Vec::with_capacity(out.len());
            let mut i = 0;
            while i < out.len() {
                if out[i..].starts_with(secret) {
                    next.extend_from_slice(placeholder);
                    i += secret.len();
                } else {
                    next.push(out[i]);
                    i += 1;
                }
            }
            out = next;
        }
        out
    }
}

impl OutputRedactor for SwappableRedactor {
    fn redact(
        &self,
        output: &ssh_manager_core::ssh::CommandOutput,
    ) -> ssh_manager_core::ssh::CommandOutput {
        let masked = ssh_manager_core::ssh::CommandOutput {
            stdout: self.mask_literals(&output.stdout),
            stderr: self.mask_literals(&output.stderr),
            exit_code: output.exit_code,
            truncated: output.truncated,
        };
        self.current().redact(&masked)
    }

    fn redact_text(&self, text: &str) -> String {
        let masked = String::from_utf8_lossy(&self.mask_literals(text.as_bytes())).into_owned();
        self.current().redact_text(&masked)
    }
}

impl AsRef<dyn OutputRedactor> for SwappableRedactor {
    fn as_ref(&self) -> &(dyn OutputRedactor + 'static) {
        self
    }
}

/// Objektsicherer Wrapper um `FilterEngine<S>::evaluate` (Aufgabenstellung
/// Teil 1, Punkt 4). `FilterEngine<S>` selbst ist über den `PolicyStore`-Typ
/// generisch, `Session` soll dieses Detail aber nicht nach außen tragen
/// (Tests wollen unterschiedliche `PolicyStore`-Implementierungen
/// einsetzen können, ohne `Session` selbst generisch machen zu müssen) —
/// ein kleines, dyn-kompatibles Trait löst das, analog zu `EventEmitter`.
///
/// `async fn` seit Spec 0009 (`PolicyStore::rules_for` liest jetzt aus der
/// SQLite-Datenbank).
///
/// Nur `evaluate_explained` (Spec 0057, §1.1: das Ledger braucht die
/// `matched_rule`/`matched_rule_origin` einer Entscheidung, nicht nur
/// deren `Decision`) — bisher gab es hier zusätzlich ein reines
/// `evaluate` ohne Erklärung, das nach der Umstellung aller Aufrufer auf
/// `evaluate_explained` (für die Ledger-`Decision`-Einträge in
/// `orchestration::evaluate_action`/`handle_user_decision`) ungenutzt
/// blieb und deshalb entfernt wurde — `Decision` ist über `.decision` auf
/// der zurückgegebenen [`EvaluationTrace`] weiterhin trivial erreichbar.
#[async_trait]
pub trait CommandEvaluator: Send + Sync {
    async fn evaluate_explained(&self, command: &str, ctx: &EvalContext) -> EvaluationTrace;

    /// Read-only evaluation as if `candidates` were additional stored rules
    /// (issue #596: verdict for a rule suggestion). Stores nothing.
    async fn evaluate_explained_with_candidates(
        &self,
        command: &str,
        ctx: &EvalContext,
        candidates: &[ssh_manager_core::filter::Rule],
    ) -> EvaluationTrace;
}

#[async_trait]
impl<S: PolicyStore + Send + Sync> CommandEvaluator for FilterEngine<S> {
    async fn evaluate_explained(&self, command: &str, ctx: &EvalContext) -> EvaluationTrace {
        FilterEngine::evaluate_explained(self, command, ctx).await
    }

    async fn evaluate_explained_with_candidates(
        &self,
        command: &str,
        ctx: &EvalContext,
        candidates: &[ssh_manager_core::filter::Rule],
    ) -> EvaluationTrace {
        FilterEngine::evaluate_explained_with_candidates(self, command, ctx, candidates).await
    }
}

/// Kommando an den Terminal-Aktor (s. Modul-Kommentar, Punkt 3).
pub enum TerminalCommand {
    Write(Vec<u8>),
    Resize(PtySize),
}

/// Spec 0085, A3: der **normale** SFTP-Kanal einer Sitzung.
///
/// Spec 0020, Abschnitt 3: lazy geöffnet (erst beim ersten
/// `ReadRemoteFile`/`WriteRemoteFile`/Dateibrowser-Zugriff, s.
/// [`crate::orchestration::ensure_sftp_open`]), danach für die Dauer der
/// Session offengehalten statt pro Zugriff neu aufgebaut. `AsyncMutex` wie
/// `transport`/`context` (über `.await`-Punkte hinweg gehalten).
///
/// **Eigener Typ mit privatem Inneren, und ein privates Feld an [`Session`]**
/// (Spec 0085, A3.1): Über diesen Kanal laufen die KI-Dateiaktionen
/// (`ReadRemoteFile`/`WriteRemoteFile`) und MCP. Könnte Code außerhalb von
/// `app-logic` ihn setzen, ließe sich ein **erhöhter** Kanal hineinschieben —
/// und KI und MCP liefen erhöht, entgegen Spec 0067 A. Befüllt wird er
/// deshalb nur hier, aus dem Transport der Sitzung selbst
/// ([`NormalSftpGuard::install`], einziger Aufrufer `ensure_sftp_open`);
/// nach außen gibt [`Session::lock_sftp`] nur eine Referenz auf das
/// Trait-Objekt heraus, und die lässt sich nicht ersetzen.
#[derive(Default)]
pub struct NormalSftpChannel {
    inner: AsyncMutex<Option<Box<dyn SftpSession>>>,
}

/// Der gesperrte normale SFTP-Kanal — s. [`Session::lock_sftp`].
pub struct NormalSftpGuard<'a>(tokio::sync::MutexGuard<'a, Option<Box<dyn SftpSession>>>);

impl NormalSftpGuard<'_> {
    /// Der Kanal, **nur zum Benutzen**.
    ///
    /// Spec 0085, A3.1: bewusst `&mut dyn SftpSession` und nicht
    /// `&mut Box<dyn SftpSession>` oder `&mut Option<…>`. Ein Trait-Objekt
    /// ist nicht `Sized`, damit scheitern `std::mem::replace`, `swap` und
    /// `take` auf dieser Referenz schon beim Kompilieren — die Grenze verläuft
    /// am Typ der herausgegebenen Referenz, nicht bloß an der Sichtbarkeit
    /// des Felds.
    ///
    /// `None`, solange niemand `ensure_sftp_open` aufgerufen hat.
    pub fn sftp(&mut self) -> Option<&mut (dyn SftpSession + 'static)> {
        self.0.as_deref_mut()
    }

    /// Wie [`Self::sftp`], aber mit einem Fehler statt `None` — für alle
    /// Aufrufer, die den Kanal **benutzen wollen** statt nur nachzusehen, ob
    /// er offen ist (Spec 0088, A3.2).
    ///
    /// Das ist die **eine** Stelle, an der ein leerer Kanal zu einem Fehler
    /// wird. Vorher stand an jedem der sechs Aufrufer in
    /// `orchestration::remote_files` ein
    /// `expect("ensure_sftp_open lief erfolgreich durch")` — sechs Kopien
    /// derselben Invariante, die keiner der Aufrufer selbst garantieren
    /// kann. Garantieren kann sie nur dieser Typ: Befüllt wird der Kanal
    /// ausschließlich über [`Self::install`], und keine Operation leert ihn
    /// wieder (s. [`NormalSftpChannel`]). Genau deshalb steht der Hinweis
    /// hier am Typ und nicht am Aufrufer — und genau deshalb endet ein
    /// künftiger Umbau, der den Kanal doch einmal leert, in einem sichtbaren
    /// Aktionsfehler im Chat statt in einem Panic mitten im Chat-Turn.
    ///
    /// Der `SshError` ist bewusst ein gewöhnlicher Kanalfehler mit Code: Die
    /// Aufrufer reichen ihn über ihren bestehenden Fehlerpfad weiter, ohne
    /// dafür einen zweiten Mechanismus zu brauchen.
    pub fn sftp_or_err(&mut self) -> Result<&mut (dyn SftpSession + 'static), SshError> {
        self.0.as_deref_mut().ok_or_else(|| {
            SshError::ChannelError("der SFTP-Kanal dieser Sitzung ist nicht geöffnet".to_string())
        })
    }

    /// Ist der Kanal schon geöffnet?
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }

    /// Spec 0085, A3.2: Befüllt den Kanal. `pub(crate)` — nur `app-logic`
    /// darf das, und dort nur `ensure_sftp_open` aus dem Transport **dieser**
    /// Sitzung.
    pub(crate) fn install(&mut self, sftp: Box<dyn SftpSession>) {
        *self.0 = Some(sftp);
    }
}

/// Issue #355: proof that this caller owns the running reconnect of a
/// session. Dropped without [`ReconnectClaim::succeed`], the session goes
/// back to `Disconnected`, so the button stays usable after a failure.
pub struct ReconnectClaim<'a> {
    session: &'a Session,
    settled: bool,
}

impl ReconnectClaim<'_> {
    /// Installs the new connection and marks the session `Connected`.
    pub async fn succeed(
        mut self,
        transport: Box<dyn SshTransport>,
        redactor: Box<dyn OutputRedactor>,
        sudo_password: Option<secrecy::SecretString>,
    ) {
        self.session
            .replace_connection(transport, redactor, sudo_password)
            .await;
        *crate::poison::lock_tolerating_poison(&self.session.status) = ConnectionStatus::Connected;
        self.settled = true;
    }
}

impl Drop for ReconnectClaim<'_> {
    fn drop(&mut self) {
        if !self.settled {
            *crate::poison::lock_tolerating_poison(&self.session.status) =
                ConnectionStatus::Disconnected;
        }
    }
}

/// Spec 0086, A3: der Transport einer Sitzung — ihre eine SSH-Verbindung.
///
/// **Eigener Typ mit privatem Inneren** statt eines nackten
/// `AsyncMutex<Box<dyn SshTransport>>`: Der normale SFTP-Kanal einer Sitzung
/// (s. [`NormalSftpChannel`]) wird aus **diesem** Transport geöffnet
/// (`ensure_sftp_open`) und danach für die Dauer der Sitzung offengehalten.
/// Ließe sich der Transport danach von außen ersetzen oder mit dem einer
/// anderen Sitzung tauschen, gehörte der schon offene Kanal zu einer
/// anderen Verbindung als die Sitzung — die Paarung „ein Transport, sein
/// Kanal" wäre gebrochen, und damit die Grundlage von Spec 0067 A (KI und
/// MCP laufen nie über den erhöhten Kanal) und Spec 0085 A3.
///
/// Nach außen gibt [`SessionTransport::lock`] deshalb nur eine Referenz auf
/// das **Trait-Objekt** heraus (`dyn SshTransport`, nicht `Sized`) — die
/// lässt sich benutzen, aber nicht ersetzen, tauschen oder entnehmen. Und
/// [`Session`] hat kein `DerefMut`, also ist auch das Feld selbst von außen
/// nur lesbar.
pub struct SessionTransport {
    inner: AsyncMutex<Box<dyn SshTransport>>,
    /// Issue #328: AI commands of one session stay serial even though the
    /// transport lock is released while a command runs. Held for the whole
    /// command by [`SessionTransport::begin_command`].
    command_guard: AsyncMutex<()>,
    /// Issue #328: set by [`SessionTransport::mark_closed`] on disconnect;
    /// no new command starts and no follow-up AI round follows afterwards.
    closed: std::sync::atomic::AtomicBool,
    /// Issue #565: counts the connections this session has held; bumped by
    /// [`SessionTransport::replace`]. A connection-loss watch only reports
    /// for the epoch it was started in, so a signal of a replaced transport
    /// never marks the new connection as lost.
    epoch: std::sync::atomic::AtomicU64,
    /// Issue #328: action currently running as an AI command, so a
    /// disconnect can cancel it (Spec 0027).
    running_action: std::sync::Mutex<Option<ActionId>>,
}

/// Issue #328: proof that this AI command is the only one running in its
/// session; dropping it lets the next one start.
pub struct CommandSlot<'a> {
    owner: &'a SessionTransport,
    _guard: tokio::sync::MutexGuard<'a, ()>,
}

impl Drop for CommandSlot<'_> {
    fn drop(&mut self) {
        *self.owner.running_action.lock().unwrap() = None;
    }
}

impl SessionTransport {
    /// Spec 0086, A3.2: Das Bauen einer Sitzung aus allen Bestandteilen
    /// bleibt erlaubt — `connect()` in `app-shell` legt den frisch
    /// aufgebauten Transport hier ab, bevor die [`Session`] entsteht.
    pub fn new(transport: Box<dyn SshTransport>) -> Self {
        Self {
            inner: AsyncMutex::new(transport),
            command_guard: AsyncMutex::new(()),
            closed: std::sync::atomic::AtomicBool::new(false),
            epoch: std::sync::atomic::AtomicU64::new(0),
            running_action: std::sync::Mutex::new(None),
        }
    }

    /// Issue #328: waits until no other AI command of this session runs and
    /// claims the slot for `action_id`. `None` if the session was closed.
    pub async fn begin_command(&self, action_id: ActionId) -> Option<CommandSlot<'_>> {
        let guard = self.command_guard.lock().await;
        if self.is_closed() {
            return None;
        }
        *self.running_action.lock().unwrap() = Some(action_id);
        Some(CommandSlot {
            owner: self,
            _guard: guard,
        })
    }

    /// Issue #328: the AI command running right now, if any.
    pub fn running_action(&self) -> Option<ActionId> {
        *self.running_action.lock().unwrap()
    }

    /// Issue #328: the session is being disconnected.
    pub fn mark_closed(&self) {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn epoch(&self) -> u64 {
        self.epoch.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Issue #355: puts the freshly connected transport of a reconnect in
    /// place of the dead one and reopens the session for commands. Only
    /// [`Session::replace_connection`] calls this, so transport and the
    /// normal SFTP channel are always replaced together.
    async fn replace(&self, transport: Box<dyn SshTransport>) {
        *self.inner.lock().await = transport;
        self.epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.closed
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Issue #328: resolves once no AI command runs in this session.
    pub async fn wait_until_idle(&self) {
        drop(self.command_guard.lock().await);
    }

    /// Der Transport, gesperrt — **zum Benutzen**.
    ///
    /// Spec 0086, A3.1/T11: Weder über diese Sperre noch über das Feld lässt
    /// sich der Transport einer bestehenden Sitzung ersetzen, entnehmen oder
    /// mit dem einer anderen Sitzung tauschen. Die folgenden Beispiele
    /// belegen das aus der Sicht eines **anderen** Crates (ein Doctest wird
    /// als eigene Kiste gegen `app_logic` gebaut); zu jedem verbotenen Fall
    /// steht der kompilierende Zwilling daneben, der sich nur in der
    /// verbotenen Zeile unterscheidet — so scheitert der Fall nachweislich an
    /// ihr und nicht an einem Tippfehler. (Der Fehlercode selbst wird von
    /// `rustdoc` nicht geprüft, deshalb dieses Zwillingspaar statt einer
    /// Codeangabe.)
    ///
    /// **Diese Fälle hängen bewusst an keinem `cfg`/Feature.** Der
    /// Test-Zugang (die `set_*_for_tests`-Setter, z. B.
    /// [`Session::set_filter_engine_for_tests`]) steht hinter
    /// `feature = "test-support"`, und dieses Feature ist im Doctest-Lauf von
    /// `cargo test --workspace` **aktiv** (Feature-Unification über
    /// `app-shell`s dev-dependency). Ein `DerefMut` hinter demselben Feature
    /// hätte die Fälle (a)–(d) unten also im Gate-Lauf leer bestehen lassen —
    /// gemessen, s. ADR 0080. Deshalb ist `DerefMut` ganz entfallen, statt
    /// nur für Tests zu bestehen.
    ///
    /// **(a)** `mem::swap` der Transporte zweier Sitzungen — verboten:
    /// ```compile_fail
    /// # use app_logic::session::Session;
    /// fn f(a: &mut Session, b: &mut Session) {
    ///     std::mem::swap(&mut a.transport, &mut b.transport);
    /// }
    /// ```
    /// Zwilling (tauscht zwei **ganze** Sitzungen, Transport und Kanal
    /// wandern zusammen — A3.2):
    /// ```
    /// # use app_logic::session::Session;
    /// fn f(a: &mut Session, b: &mut Session) {
    ///     std::mem::swap(a, b);
    /// }
    /// ```
    ///
    /// **(b)** `mem::replace` des Transports — verboten:
    /// ```compile_fail
    /// # use app_logic::session::{Session, SessionTransport};
    /// fn f(session: &mut Session, replacement: SessionTransport) {
    ///     let _old = std::mem::replace(&mut session.transport, replacement);
    /// }
    /// ```
    /// Zwilling:
    /// ```
    /// # use app_logic::session::{Session, SessionTransport};
    /// fn f(session: &mut Session, replacement: SessionTransport) {
    ///     let _old = replacement;
    /// }
    /// ```
    ///
    /// **(c)** Tausch **aller mitgegebenen Bestandteile** zweier Sitzungen —
    /// verboten (nimmt den Transport mit, ließe aber den privaten SFTP-Kanal
    /// zurück):
    /// ```compile_fail
    /// # use app_logic::session::Session;
    /// fn f(a: &mut Session, b: &mut Session) {
    ///     std::mem::swap(&mut **a, &mut **b);
    /// }
    /// ```
    /// Zwilling (ein `*` weniger je Seite: zwei ganze `Session`-Werte):
    /// ```
    /// # use app_logic::session::Session;
    /// fn f(a: &mut Session, b: &mut Session) {
    ///     std::mem::swap(&mut *a, &mut *b);
    /// }
    /// ```
    ///
    /// **(d)** Zuweisung eines neuen Transports — verboten:
    /// ```compile_fail
    /// # use app_logic::session::{Session, SessionTransport};
    /// fn f(session: &mut Session, replacement: SessionTransport) {
    ///     session.transport = replacement;
    /// }
    /// ```
    /// Zwilling:
    /// ```
    /// # use app_logic::session::{Session, SessionTransport};
    /// fn f(session: &mut Session, replacement: SessionTransport) {
    ///     drop(replacement);
    /// }
    /// ```
    ///
    /// **(e1)** Schon mit **nur `&Session`** (also auch hinter dem `Arc` in
    /// der Registry): Ersetzen des Transport-Werts über die Sperre —
    /// verboten, `dyn SshTransport` ist nicht `Sized`:
    /// ```compile_fail
    /// # use app_logic::session::Session;
    /// # use ssh_manager_core::ssh::SshTransport;
    /// async fn f(session: &Session, replacement: Box<dyn SshTransport>) {
    ///     let mut guard = session.transport.lock().await;
    ///     let _old = std::mem::replace(&mut *guard, replacement);
    /// }
    /// ```
    /// Zwilling (benutzt den Transport über die Sperre — A3.2):
    /// ```
    /// # use app_logic::session::Session;
    /// # use ssh_manager_core::ssh::SshTransport;
    /// async fn f(session: &Session, replacement: Box<dyn SshTransport>) {
    ///     let mut guard = session.transport.lock().await;
    ///     let _old = guard.execute("true").await;
    /// }
    /// ```
    ///
    /// **(e2)** Mit nur `&Session`: `mem::swap` der Inhalte zweier gesperrter
    /// Transporte — verboten:
    /// ```compile_fail
    /// # use app_logic::session::Session;
    /// async fn f(a: &Session, b: &Session) {
    ///     let mut ga = a.transport.lock().await;
    ///     let mut gb = b.transport.lock().await;
    ///     std::mem::swap(&mut *ga, &mut *gb);
    /// }
    /// ```
    /// Zwilling (benutzt beide Transporte, tauscht sie nur nicht):
    /// ```
    /// # use app_logic::session::Session;
    /// async fn f(a: &Session, b: &Session) {
    ///     let mut ga = a.transport.lock().await;
    ///     let mut gb = b.transport.lock().await;
    ///     let _ = (ga.execute("true").await, gb.execute("true").await);
    /// }
    /// ```
    pub async fn lock(&self) -> TransportGuard<'_> {
        TransportGuard(self.inner.lock().await)
    }
}

/// Der gesperrte Transport einer Sitzung — s. [`SessionTransport::lock`].
///
/// Spec 0086, A3.2: `Deref`/`DerefMut` auf `dyn SshTransport` (nicht auf
/// `Box<dyn SshTransport>`). Alle Methoden von [`SshTransport`] bleiben
/// dadurch unverändert über die Sperre erreichbar (`guard.execute(…)`),
/// aber `mem::replace`/`swap`/`take` scheitern an der Größe des
/// Trait-Objekts — dasselbe Muster wie [`NormalSftpGuard::sftp`].
pub struct TransportGuard<'a>(tokio::sync::MutexGuard<'a, Box<dyn SshTransport>>);

impl std::ops::Deref for TransportGuard<'_> {
    type Target = dyn SshTransport + 'static;
    fn deref(&self) -> &Self::Target {
        &**self.0
    }
}

impl std::ops::DerefMut for TransportGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut **self.0
    }
}

/// Eine laufende Server-Session.
///
/// Spec 0085, A3: Die Struktur ist in zwei Teile getrennt. Alles, was der
/// Aufrufer beim Verbinden mitgibt, steht in [`SessionParts`] und ist über
/// `Deref` unverändert als `session.<feld>` **lesbar**. Der normale
/// SFTP-Kanal dagegen ist ein **privates** Feld: er wird nie übergeben,
/// sondern startet leer und wird nur hier befüllt (s.
/// [`NormalSftpChannel`]).
///
/// Spec 0086, A3: **kein `DerefMut`.** Mit einem `DerefMut` ließe sich von
/// außen jedes mitgegebene Feld überschreiben — auch der Transport
/// (`session.transport = …`), und über `mem::swap(&mut **a, &mut **b)` sogar
/// alle mitgegebenen Bestandteile zweier Sitzungen auf einmal, während der
/// private SFTP-Kanal zurückbliebe. Ohne `DerefMut` ist von außen kein Feld
/// einer bestehenden Sitzung schreibbar; Tests bekommen den Zugang über
/// feldweise `set_<feld>_for_tests`-Setter hinter
/// `cfg(test)`/`feature = "test-support"` (A3.3, Muster wie
/// [`Session::set_sftp_for_tests`]).
pub struct Session {
    parts: SessionParts,
    sftp: NormalSftpChannel,
    /// Spec 0102: konfiguriertes Startverzeichnis und das einmalige
    /// Prüfergebnis dieser Sitzung (s. `crate::start_directory`). Privat
    /// wie `sftp`: gesetzt nur über [`Session::with_start_directory`] beim
    /// Bau, danach nur lesbar.
    start_directory: crate::start_directory::SessionStartDirectory,
    /// Issue #544: result of the connect-time `id -u` probe. `Some(true)` =
    /// uid 0, `None` = unknown (probe failed or not run). UI hint only.
    is_root: Option<bool>,
    /// Issue #610: where completed AI requests are reported (observers an
    /// edition registered). `None` = nobody listens. Private: set at build.
    usage_reporting: Option<crate::ai_usage::AiUsageReporting>,
}

impl std::ops::Deref for Session {
    type Target = SessionParts;
    fn deref(&self) -> &SessionParts {
        &self.parts
    }
}

/// Alle Bestandteile einer [`Session`], die beim Verbinden mitgegeben werden
/// — der normale SFTP-Kanal gehört bewusst **nicht** dazu (Spec 0085, A3:
/// „Der Konstruktor nimmt keinen Kanal an", ein neu erzeugter Kanal ist
/// immer leer).
pub struct SessionParts {
    /// Spec 0086, A3: eigener Typ statt `AsyncMutex<Box<dyn SshTransport>>`
    /// — s. [`SessionTransport`] zur Begründung.
    pub transport: SessionTransport,
    pub ai_provider: Box<dyn AiProvider>,
    /// Spec 0061: der Rate-Limit-Budget-Wächter für `ai_provider` (aus
    /// `AppState.rate_limit_registry` bei `connect()` aufgelöst, s.
    /// `risk_second_opinion_budget`-Doc-Kommentar zur Begründung, warum
    /// dieses separate Feld nötig ist).
    pub ai_provider_budget: Arc<ai_providers::ProviderBudgetGuard>,
    pub context: AsyncMutex<SessionContext>,
    pub filter_engine: Box<dyn CommandEvaluator>,
    pub server_id: ServerId,
    pub tags: Vec<String>,
    /// `Some`, sobald `open_terminal` den Aktor gestartet hat.
    /// `terminal_input`/`terminal_resize` senden darüber; `None` (noch kein
    /// `open_terminal` aufgerufen, oder Kanal bereits geschlossen) wird als
    /// Anwenderfehler zurückgemeldet statt zu blockieren.
    pub terminal: StdMutex<Option<mpsc::UnboundedSender<TerminalCommand>>>,
    /// Läuft über jeden `SshTransport::execute()`-Output, bevor er in
    /// `chat-action-result` und `context.history` landet (Spec 0006,
    /// Abschnitt 5) — pro Session eine Instanz statt bei jeder Ausführung
    /// neu aufgebaut (baut intern eine feste Regex-Liste auf).
    pub redactor: SwappableRedactor,
    /// Für `NoteEditor::Ai { provider, model }` bei `ProposeNoteUpdate`
    /// (Spec 0003, Abschnitt 5.3) — aus der aktiven `AiProviderConfig` zum
    /// Verbindungszeitpunkt übernommen, damit spätere Notiz-Historie
    /// nachvollziehbar bleibt, welcher Provider/welches Modell den
    /// Vorschlag gemacht hat.
    pub ai_provider_label: String,
    pub ai_model: String,
    /// Spec 0057, §4.1: die ungefencten Rohbestandteile des aktuellen
    /// System-Prompts (Basis-Text, Notiz-Sektionen nach Scope geordnet —
    /// seit Spec 0064 OHNE die Remote-OS-Info, die lebt jetzt als eigene,
    /// gefencte Verlaufs-Nachricht, s. `crate::compaction::
    /// SystemContextParts`-Doc-Kommentar) — parallel zu `context.lock()
    /// .await.system_context`
    /// (dem bereits zusammengesetzten String) gepflegt, s.
    /// `crate::compaction::SystemContextParts`-Doc-Kommentar zur
    /// Begründung, warum das getrennt statt nur aus dem fertigen String
    /// gehalten wird. Aktualisiert an denselben zwei Stellen wie `context`:
    /// `app_shell::commands::connect_session` (initial) und
    /// `send_chat_message_impl` (bei jeder Nutzer-Nachricht neu gebaut).
    pub system_context_parts: AsyncMutex<crate::compaction::SystemContextParts>,
    /// Spec 0057, §3.1: geschätztes Kontextfenster (Token) des bei
    /// `connect()` aktiven KI-Providers/-Modells — einmalig aufgelöst über
    /// `crate::compaction::model_context_window_tokens`, analog zu
    /// `ai_provider_label`/`ai_model` oben (ein während der Sitzung
    /// geänderter aktiver Provider greift wie dort erst bei der nächsten
    /// `connect()`).
    pub model_context_window_tokens: usize,
    /// Spec 0057, §2: die aktuelle rollierende Zusammenfassung alter,
    /// bereits aus dem KI-Kontext verdichteter Gesprächsrunden — `None`,
    /// solange Schritt 1 der Kompaktierungs-Leiter (`compaction::
    /// compact_for_send`) noch nie greifen musste, oder nach einem
    /// wiederaufgenommenen Resume ohne zuvor gespeicherte Summary.
    /// Aktualisiert ausschließlich von `compaction::
    /// compact_rounds_with_summary` (nie direkt), persistiert
    /// wie die Chat-Historie/das Ledger, geschützt durch die Verschlüsselung
    /// der Datenbankdatei (Spec 0036, Spec 0057 §2.3),
    /// s. `persistence_sqlite::SqliteChatSessionStore::save_summary`.
    pub summary: AsyncMutex<Option<crate::compaction::RollingSummary>>,
    /// Issue #601: real input sizes reported by the provider, used to
    /// correct the compaction estimate (in memory, per session).
    pub context_usage: StdMutex<crate::compaction::ContextUsageTracker>,
    /// MCP-Ausschluss aus der rollierenden Summary (Nachtrag zu Spec 0057
    /// §2.1, Entscheidung: die Summary bildet Chat-Kontinuität ab,
    /// MCP-Verkehr ist kein Chat — Audit bleibt vollständig im Ledger,
    /// Etappe 1, unabhängig davon). Parallel zu `context.history` geführt
    /// (Index `i` sagt, ob `context.history[i]` MCP-originiert ist,
    /// `true` ⟺ die Nachricht wurde mit `persist: false` gepusht) —
    /// `ChatMessage` selbst trägt keine Herkunftsinformation (kein
    /// `core`-Typ soll App-Shell-Konzepte wie MCP kennen), deshalb dieser
    /// Session-lokale Spiegel. Ausschließlich von `orchestration::
    /// push_history_scoped` gepflegt, IM SELBEN `context`-Lock wie der
    /// `history.push` selbst (atomar, keine zwei Nachrichten können sich
    /// zwischen den beiden Pushes einschieben) — dieselbe Quelle der
    /// Wahrheit wie der bereits bestehende `persist`-Parameter, der auch
    /// den `chat_messages`-Persistenz-Ausschluss steuert (Spec 0034 §10),
    /// hier nur zusätzlich für die Kompaktierung gespiegelt statt eines
    /// zweiten, divergierenden Mechanismus. `StdMutex` statt `AsyncMutex`:
    /// nur kurze, nie über einen `.await`-Punkt gehaltene Push-Operationen,
    /// wie bei `pending_action`.
    pub mcp_origin_flags: StdMutex<Vec<bool>>,
    /// Spec 0018, Abschnitt 6: optionales Sudo-Passwort, einmalig bei
    /// `connect()` aus dem `CredentialStore` gelesen — `None`, wenn für den
    /// Server keines hinterlegt ist (kein Fehler, s. dortiger Kommentar).
    /// Wird ausschließlich über Stdin an genau einen `sudo -S`-Aufruf
    /// weitergereicht (`crate::orchestration::execute_suggested_command`),
    /// nie auf dem Zielserver abgelegt.
    pub sudo_password: SwappableSudoPassword,
    /// Spec 0017, Abschnitt 2: `list_sessions()`-Statusfeld. Startet bei
    /// `Connected` (`Session` wird erst nach erfolgreichem Verbindungsaufbau
    /// konstruiert, s. `app_shell::commands::connect`) und wird von
    /// `spawn_terminal_actor` auf `Disconnected` gesetzt, sobald der
    /// Terminal-Aktor unerwartet endet (Netzwerkfehler/Verbindungsabbruch) —
    /// ein expliziter `disconnect()`-Aufruf entfernt die Session ohnehin
    /// komplett aus `SessionManager`, dort ist kein Status-Update nötig.
    /// Eigener `StdMutex` statt z. B. eines `AtomicU8`: `ConnectionStatus`
    /// ist ein einfaches Copy-Enum, aber kein Integer — ein Mutex bleibt
    /// hier die geradlinigste Wahl, analog zu `terminal` oben.
    pub status: StdMutex<ConnectionStatus>,
    /// Spec 0017, Abschnitt 5: `Some(action_id)`, während diese Session auf
    /// eine `Confirm`-Bestätigung wartet (gesetzt in
    /// `crate::orchestration::handle_action_proposed`, gelöscht sobald die
    /// wartende `rx.await` dort zurückkehrt) — die Grundlage für
    /// `SessionSummaryDto.has_pending_action`, den Tab-Indikator im
    /// Frontend. **Nicht** für den Disconnect-Notiz-Vorschlag (Spec 0010)
    /// verwendet: der läuft bewusst erst, nachdem die Session bereits aus
    /// `SessionManager` entfernt wurde, und ist als App-weite Benachrichtigung
    /// (`note-update-suggested`) ohnehin nie an einen Tab gebunden.
    pub pending_action: StdMutex<Option<ActionId>>,
    /// Issue #108, Spec 0104 §3: `true`, solange eine MCP-Bestätigung dieser
    /// Session registriert ist (von der Registrierung bis zum Aufräumen,
    /// also auch vor dem Setzen des Indikators). Ein weiterer MCP-Vorschlag
    /// wird dann abgewiesen.
    pub mcp_confirmation_claim: std::sync::atomic::AtomicBool,
    // Spec 0085, A3: Der normale SFTP-Kanal war bis hierhin ein `pub`-Feld
    // dieser Struktur. Er liegt jetzt als privates Feld an `Session` selbst
    // (s. `NormalSftpChannel`): so kann Code außerhalb von `app-logic` ihn
    // weder setzen noch ersetzen noch herausnehmen, sondern nur benutzen.
    // Spec 0067, Teil A: Der erhöhte SFTP-Kanal (`sudo -n <sftp-server>`)
    // war bis Spec 0084 ein Feld dieser Struktur. Er liegt jetzt in
    // `app_shell::elevated_sftp::ElevatedSftpRegistry`, einem eigenen, von Tauri
    // verwalteten Zustand von `app-shell` (Spec 0084, A1): so führt von
    // einer `Session` aus kein Weg mehr zu ihm, auch nicht für Code, der
    // eine `Session` in der Hand hat.
    /// Spec 0021, Abschnitt 5 / Spec 0066, §1: gesetzt über
    /// [`Session::request_auto_continue_stop`] (`app_shell::commands::
    /// stop_auto_continuation`). Beendet die Fortsetzungskette und bricht
    /// einen laufenden KI-Stream bzw. die Wartezeit vor dem Send sofort ab
    /// (`crate::orchestration::run_one_round`); ein offener
    /// Bestätigungsdialog und ein bereits laufendes Kommando bleiben
    /// unangetastet. Zurückgesetzt in `app_shell::commands::
    /// send_chat_message_impl` beim Start eines Turns, atomar mit
    /// `chat_turn.running`.
    pub auto_continue_stop: std::sync::atomic::AtomicBool,
    /// Spec 0066, §1: weckt eine laufende Runde auf, sobald
    /// `auto_continue_stop` gesetzt wird, damit ein laufender KI-Stream
    /// sofort abgebrochen wird statt erst an der nächsten Rundengrenze.
    /// Immer über [`Session::request_auto_continue_stop`] setzen.
    pub auto_continue_stop_notify: tokio::sync::Notify,
    /// Spec 0066, §2: ob gerade ein Chat-Turn läuft, und welche
    /// Nutzer-Nachrichten währenddessen eingereiht wurden. Beides unter
    /// EINEM Lock, damit keine Nachricht zwischen "Warteschlange leer" und
    /// "Turn beendet" verloren gehen kann.
    pub chat_turn: StdMutex<ChatTurnState>,
    /// Spec 0026, Abschnitt 3: `Some`, wenn die optionale KI-Zweitmeinung
    /// zur Daten-Risiko-Achse aktiviert ist UND ein gültiger, separater
    /// Provider dafür konfiguriert ist — einmalig bei `connect()` aus den
    /// `tauri-plugin-store`-Einstellungen aufgelöst (analog zu
    /// `ai_provider_label`/`ai_model` oben), nicht bei jedem Aktionsvorschlag
    /// neu gelesen. Ein während einer laufenden Session geänderter Wert
    /// greift dadurch erst bei der nächsten `connect()` — ein bewusster,
    /// kleiner Scope-Kompromiss: eine Live-Aktualisierung mitten in einer
    /// Session hätte deutlich mehr Zustands-Plumbing gebraucht, für eine
    /// reine Komfort-Einstellung unverhältnismäßig.
    pub risk_second_opinion_provider: Option<Box<dyn AiProvider>>,
    /// Spec 0061: derselbe Rate-Limit-Budget-Wächter, den der zugehörige
    /// `risk_second_opinion_provider` beim Bau (`ai_provider_factory::
    /// build_ai_provider`) aus `AppState.rate_limit_registry` bekommen hat
    /// — als eigenständiges Feld gehalten, weil `Box<dyn AiProvider>`
    /// (s. oben) keinen Weg bietet, nachträglich an seinen internen
    /// Wächter heranzukommen (die `AiProvider`-Trait-Grenze kennt Spec
    /// 0061 nicht, s. `crate::orchestration::wait_for_rate_limit_budget`,
    /// das dieses Feld VOR dem jeweiligen `send()`-Aufruf konsultiert).
    /// `Some` genau dann, wenn `risk_second_opinion_provider` `Some` ist.
    pub risk_second_opinion_budget: Option<Arc<ai_providers::ProviderBudgetGuard>>,
    /// Spec 0092, A1.3: ob ein rot eingestufter Vorschlag immer eine
    /// Bestätigung verlangt — app-weite Einstellung (`redRiskAlwaysConfirm`
    /// in `settings.json`), einmalig bei `connect()` aufgelöst, genau wie
    /// `risk_second_opinion_provider` oben. Eine Änderung greift deshalb
    /// erst bei der nächsten Verbindung; der Hinweistext am Schalter sagt
    /// das.
    ///
    /// **Fail-safe `true`** (A1.2): fehlender Schlüssel, kein boolescher
    /// Wert oder ein nicht öffenbarer Store gelten als „an". In
    /// Test-Fixtures ebenfalls `true` (Spec 0092, A1.2) — damit ein
    /// bestehender Test mit rotem Kommando und Allow-Regel sichtbar bricht,
    /// statt still auf „aus" zu laufen.
    pub red_risk_always_confirm: bool,
    /// Spec 0027: derselbe `Arc` wie `AppState.running_command_
    /// cancellations` — ein billiger Klon bei `connect()`, damit
    /// `orchestration::execute_suggested_command` (die nur `&Session`
    /// bekommt, kein `AppState`) einen laufenden Abbruch registrieren kann,
    /// ohne die Signaturen von `run_chat_turn`/`run_one_round`/
    /// `handle_action_proposed` anfassen zu müssen (s. Doc-Kommentar auf
    /// `AppState.running_command_cancellations`).
    pub running_command_cancellations: Arc<ConfirmationRegistry<ActionId, ()>>,
    /// Issue #464 (Spec 0038): the edition's transform for approved text right
    /// before it is sent (`AppState.outgoing_transform`). `None`: text is sent
    /// unchanged.
    pub outgoing_transform: Option<Arc<dyn ssh_manager_core::outgoing::OutgoingTransform>>,
    /// Issue #505: chat content extensions (`AppState.chat_content`); the
    /// send path seals extension parts through it.
    pub chat_content: Arc<crate::chat_content_extension::ChatContentRegistry>,
    /// Spec 0039, Abschnitt 5: `true`, sobald in dieser Sitzung
    /// **irgendein** durch `ai::fence_untrusted` gelaufener Inhalt in den
    /// KI-Kontext gelangt ist (Kommando-Ausgabe, SFTP-Dateiinhalt oder
    /// Server-/Gruppen-Notiz im System-Prompt) — anders als
    /// `auto_continue_stop` **niemals** zurückgesetzt, auch nicht über neue
    /// Nutzer-Nachrichten hinweg (ersetzt damit die alte, pro-Turn
    /// zurückgesetzte SEC-03-Bremse, s. `orchestration::handle_action_
    /// proposed`-Kommentar an der Eskalationsstelle für die Begründung).
    /// Initialisiert mit `true`, wenn die Sitzung bereits mit Notizen oder
    /// (künftig, Spec 0034) vorbelasteter Historie startet — s.
    /// `history_contains_untrusted_content`.
    pub untrusted_content_ingested: std::sync::atomic::AtomicBool,
    /// Spec 0039, Abschnitt 5.1: einmalig bei `connect()` vom Server-Profil
    /// übernommen (analog zu `ai_provider_label`/`risk_second_opinion_
    /// provider` oben) — steuert, wie stark eskaliert wird, NACHDEM
    /// `untrusted_content_ingested` gesetzt ist. Das Fencing selbst
    /// (Abschnitt 3/4) läuft davon unabhängig immer.
    pub post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy,
    /// Spec 0039, Abschnitt 5.2: `Some`, wenn sowohl die app-weite
    /// Zweitmeinungs-Einstellung (Spec 0026, Abschnitt 3) als auch die
    /// serverspezifische `ai_injection_check_enabled`-Einstellung aktiv
    /// sind — einmalig bei `connect()` aufgelöst (analog zu
    /// `risk_second_opinion_provider`, derselbe konfigurierte Provider,
    /// separat aufgelöst, weil `Box<dyn AiProvider>` nicht `Clone` ist).
    pub injection_check_provider: Option<Box<dyn AiProvider>>,
    /// Spec 0061: Gegenstück zu `risk_second_opinion_budget`, für
    /// `injection_check_provider`. In der Praxis oft **derselbe** `Arc`
    /// wie `risk_second_opinion_budget` (beide werden normalerweise aus
    /// derselben `riskClassifierProviderId`-Einstellung gebaut, s.
    /// `risk_second_opinion::resolve_second_opinion_provider` — die
    /// `RateLimitRegistry` gibt für dieselbe Provider-Identität denselben
    /// Wächter zurück, unabhängig vom Aufrufzweck, s. Spec 0061 Abschnitt
    /// 2, "geteiltes Budget pro Provider-Identität").
    pub injection_check_budget: Option<Arc<ai_providers::ProviderBudgetGuard>>,
    /// Spec 0039, Abschnitt 5.2: `true`, sobald der letzte gelaufene
    /// Einschleusungs-Check "ja" ergeben hat — anders als
    /// `untrusted_content_ingested` NICHT dauerhaft-monoton, sondern
    /// "klebrig bis verbraucht": `handle_action_proposed` setzt es beim
    /// Eskalieren einer Aktion wieder auf `false` zurück, weil sich die
    /// Eskalation laut Spec auf "die auf diesem Inhalt basierende
    /// Folgeaktion" bezieht (Singular), nicht auf den Rest der Sitzung.
    pub injection_suspected: std::sync::atomic::AtomicBool,
    /// Issue #102: `true`, wenn der letzte Einschleusungs-Check nicht laufen
    /// konnte (Provider-Fehler) oder kein Urteil lieferte. Wie
    /// `injection_suspected` "klebrig bis verbraucht": die nächste
    /// `AutoExec`-Aktion wird zu `Confirm` und setzt es zurück (fail closed,
    /// pro Aktion). Beeinflusst `injection_suspected` nie.
    pub injection_check_unavailable: std::sync::atomic::AtomicBool,
    /// Issue #102: `true`, wenn die Zweitmeinung in den Einstellungen aktiv
    /// ist, beim `connect()` aber nicht eingerichtet werden konnte. Der
    /// Sitzungs-Hinweis wird einmalig gesendet (`swap(false)`).
    pub second_opinion_setup_notice_pending: std::sync::atomic::AtomicBool,
    /// Issue #231: `true`, wenn die serverspezifische Injection-Prüfung
    /// eingeschaltet ist, beim `connect()` aber kein Anbieter zur Verfügung
    /// stand (Zweitmeinung app-weit aus oder nicht einrichtbar). Nur
    /// Hinweis, keine erzwungene Bestätigung; einmalig gesendet
    /// (`swap(false)`).
    pub injection_check_inactive_notice_pending: std::sync::atomic::AtomicBool,
    /// Spec 0034: Persistenz-Anbindung für diese Sitzung. `None` für
    /// Sitzungen, die bewusst keine `chat_sessions`-Zeile bekommen sollen
    /// (Tests; s. Abschnitt 10 zu MCP — MCP-ausgelöste Aktionen laufen
    /// ohnehin nie über `run_chat_turn`/eine eigene `Session`, s.
    /// `app_shell::mcp_backend`, insofern betrifft dieses Feld sie gar nicht
    /// erst). Konkreter Store-Typ statt Trait-Abstraktion — derselbe
    /// Präzedenzfall wie `SqlitePromptHistoryStore` in `AppState` (Spec
    /// 0015): kein `core`-Trait für diese Art Hilfs-Store, anders als
    /// `ProfileStore`/`PolicyStore`/`AiProvider`.
    pub chat_session_store: Option<persistence_sqlite::SqliteChatSessionStore>,
    /// Spec 0057, §1: das Session-Ledger dieser Sitzung. Bewusst
    /// UNABHÄNGIG vom `persist`-Flag von `push_history_scoped` (das MCP-
    /// Herkunft von `chat_messages` ausschließt, Spec 0034/0040) — das
    /// Ledger soll MCP-Aktivität gerade erfassen (Spec 0057, §1.1: Quelle
    /// `mcp-agent`). Gleiches `None`-Verhalten wie `chat_session_store`
    /// (Tests; Sitzungen ohne eigene `chat_sessions`-Zeile).
    pub ledger_store: Option<persistence_sqlite::SqliteLedgerStore>,
    /// Die `chat_sessions.id`-Zeile dieser laufenden Sitzung — `None`, bis
    /// `app_shell::commands::connect_session` sie anlegt (bzw. bei
    /// `resume_chat_session`, Teil 2, auf die wiederverwendete Zeile
    /// gesetzt wird). `AsyncMutex` statt `StdMutex`, weil
    /// `history_push_and_persist` sie über einen `.await`-Punkt hinweg
    /// hält (der eigentliche `INSERT`).
    pub chat_session_id: AsyncMutex<Option<uuid::Uuid>>,
    /// Spec 0051, Teil 2: Zeitpunkt des letzten `AiProvider::send()`-
    /// Aufrufs dieser Sitzung — über alle KI-Anfrage-Arten hinweg
    /// (Haupt-Chat, Risiko-Zweitmeinung, Einschleusungs-Check, Auto-Titel,
    /// Notiz-Vorschlag) und unabhängig von der konkreten `AiProvider`-
    /// Instanz: `risk_second_opinion_provider`/`injection_check_provider`
    /// können denselben API-Key/dasselbe Rate-Limit-Kontingent wie
    /// `ai_provider` teilen, ohne dass die App das unterscheiden könnte —
    /// s. `crate::orchestration::wait_for_ai_request_slot`, das dieses
    /// Feld unmittelbar vor jedem `send()`-Aufruf konsultiert. `None` vor
    /// dem ersten Aufruf.
    pub ai_request_paced_at: AsyncMutex<Option<tokio::time::Instant>>,
    /// Issue #356, Spec 0107 §8: state of the automatic reconnect.
    pub auto_reconnect: crate::auto_reconnect::AutoReconnectState,
}

/// Spec 0066, §2 — s. `Session::chat_turn`.
#[derive(Debug, Default)]
pub struct ChatTurnState {
    pub running: bool,
    pub queued: Vec<String>,
    /// Issue #315: `true`, solange „Neuer Chat" den Turn-Slot hält. Nutzer-
    /// Nachrichten werden dann abgelehnt statt eingereiht (sonst landeten sie
    /// unbemerkt in der ersten Anfrage des neuen Chats).
    pub swapping: bool,
}

impl Session {
    /// Id handed to every `ChatContentExtension` hook (Issue #620): the saved
    /// chat session id, which survives a resume under a new tab id. Only a
    /// session without a saved chat (persistence off, local server) falls
    /// back to the tab id; such content can never be resumed anyway.
    pub async fn extension_content_key(&self, tab_id: SessionId) -> uuid::Uuid {
        (*self.chat_session_id.lock().await).unwrap_or(tab_id)
    }

    /// Spec 0085, A3: der einzige Weg, eine `Session` zu bauen. Der normale
    /// SFTP-Kanal startet immer leer; er wird beim ersten Zugriff aus dem
    /// Transport **dieser** Sitzung geöffnet (`ensure_sftp_open`).
    pub fn new(parts: SessionParts) -> Self {
        Self {
            parts,
            sftp: NormalSftpChannel::default(),
            start_directory: Default::default(),
            is_root: None,
            usage_reporting: None,
        }
    }

    /// Issue #610: sets up reporting of completed AI requests to observers.
    /// Only at construction. An empty observer list is the same as none.
    pub fn with_usage_reporting(
        mut self,
        reporting: Option<crate::ai_usage::AiUsageReporting>,
    ) -> Self {
        self.usage_reporting = reporting.filter(|r| !r.observers.is_empty());
        self
    }

    /// Issue #610: tells the observers about one completed AI request of
    /// this session. Passive: returns nothing, never waits for an observer,
    /// carries no content. Call it when the provider's `RequestUsage` arrived
    /// before the stream loop stopped; never wait for it.
    pub async fn report_ai_usage(
        &self,
        purpose: crate::ai_usage::AiUsagePurpose,
        usage: ssh_manager_core::ai::AiUsage,
    ) {
        let Some(reporting) = &self.usage_reporting else {
            return;
        };
        let Some(source) = reporting.source_for(purpose) else {
            return;
        };
        let chat_session_id = (*self.chat_session_id.lock().await).map(|id| id.to_string());
        let record = crate::ai_usage::AiUsageRecord {
            chat_session_id,
            runtime_session_id: reporting.runtime_session_id.clone(),
            server_id: (!crate::dto::is_local(self.server_id))
                .then(|| self.server_id.0.to_string()),
            provider_config_id: source.provider_config_id.clone(),
            provider_type: source.provider_type,
            configured_model: source.configured_model.clone(),
            purpose,
            usage,
            completed_at: chrono::Utc::now(),
        };
        crate::ai_usage::notify(&reporting.observers, record);
    }

    /// Issue #544: records the `id -u` probe result; only at construction.
    pub fn with_is_root(mut self, is_root: Option<bool>) -> Self {
        self.is_root = is_root;
        self
    }

    /// Issue #544: `Some(true)` if the login user is root, `None` if unknown.
    pub fn is_root(&self) -> Option<bool> {
        self.is_root
    }

    /// Spec 0102: setzt das Startverzeichnis für Terminal und Dateibrowser
    /// dieser Sitzung — nur beim Bau, bevor die Sitzung geteilt wird.
    /// `None` = Home wie bisher. Der KI-Ausführungspfad liest diesen Wert
    /// nie.
    pub fn with_start_directory(mut self, configured: Option<String>) -> Self {
        self.start_directory = crate::start_directory::SessionStartDirectory::new(configured);
        self
    }

    pub(crate) fn start_directory_state(&self) -> &crate::start_directory::SessionStartDirectory {
        &self.start_directory
    }

    /// Issue #355, Spec 0105: claims the reconnect of a disconnected
    /// session. Only a session in `Disconnected` state can be claimed, and
    /// only once: the state flips to `Reconnecting` under the lock, so a
    /// second trigger (double click, `R` key held down) gets `None`.
    pub fn begin_reconnect(&self) -> Option<ReconnectClaim<'_>> {
        let mut status = crate::poison::lock_tolerating_poison(&self.status);
        if *status != ConnectionStatus::Disconnected {
            return None;
        }
        *status = ConnectionStatus::Reconnecting;
        Some(ReconnectClaim {
            session: self,
            settled: false,
        })
    }

    /// Issue #355: retires the dead connection before a reconnect: no new
    /// command starts, a running AI command is told to stop (by the
    /// caller, via `running_action`), the old transport is closed on a
    /// best-effort basis.
    pub async fn retire_connection(&self) {
        self.transport.mark_closed();
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            self.transport.wait_until_idle(),
        )
        .await;
        let _ = self.transport.lock().await.disconnect().await;
        *crate::poison::lock_tolerating_poison(&self.terminal) = None;
    }

    /// Issue #355: replaces transport, normal SFTP channel and redactor of
    /// this session in place. Tab, session id, chat history, AI context,
    /// summary and system context parts stay untouched. The normal SFTP
    /// channel is emptied (it belonged to the old transport) and reopens
    /// lazily from the new one. Elevated state lives outside the session
    /// and is dropped by the caller.
    async fn replace_connection(
        &self,
        transport: Box<dyn SshTransport>,
        redactor: Box<dyn OutputRedactor>,
        sudo_password: Option<secrecy::SecretString>,
    ) {
        *self.sftp.inner.lock().await = None;
        self.transport.replace(transport).await;
        // One read feeds both: the password sent to `sudo -S` and the
        // redactor always agree.
        self.sudo_password.replace(sudo_password);
        self.redactor.replace(redactor);
    }

    /// Replaces the redactor of the open session (an extension's secret
    /// values changed). History is re-redacted on the next send.
    ///
    /// Literal secret values (issue #464, added by placeholder substitution)
    /// are **kept**: this is the swap for anything but an extension rebuild.
    pub fn swap_redactor(&self, redactor: Box<dyn OutputRedactor>) {
        self.redactor.replace(redactor);
    }

    /// Issue #635: like [`Self::swap_redactor`], but the session's literal
    /// secret values are **replaced** by `literals` (the extension's
    /// currently known secrets), so a withdrawn value is no longer masked.
    /// Only the extension rebuild path uses this; reconnect keeps literals.
    /// Already redacted stored text is not re-read.
    pub fn swap_redactor_with_literals(
        &self,
        redactor: Box<dyn OutputRedactor>,
        literals: &[secrecy::SecretString],
    ) {
        self.redactor.replace_with_literals(redactor, literals);
    }

    /// Der normale SFTP-Kanal dieser Sitzung, gesperrt — **zum Benutzen**.
    ///
    /// Spec 0085, A3.1/T11: Weder über diese Referenz noch über das Feld
    /// lässt sich der Kanal setzen, ersetzen oder herausnehmen. Die folgenden
    /// Beispiele belegen das aus der Sicht eines **anderen** Crates (ein
    /// Doctest wird als eigene Kiste gegen `app_logic` gebaut); zu jedem
    /// verbotenen Fall steht der kompilierende Zwilling daneben, der sich nur
    /// in der verbotenen Zeile unterscheidet — so scheitert der Fall
    /// nachweislich an ihr und nicht an einem Tippfehler. (Der Fehlercode
    /// selbst wird von `rustdoc` nicht geprüft, deshalb dieses Zwillingspaar
    /// statt einer Codeangabe.)
    ///
    /// **Spec 0086, A3.4:** Die verbotenen Fälle sind unverändert
    /// `compile_fail`. Ihre kompilierenden Zwillinge schrieben bisher ein
    /// anderes, öffentliches Feld (`session.tags = …`) — das ging nur über
    /// `DerefMut`, und das ist mit Spec 0086 A3 entfallen. Sie führen die
    /// jeweilige Mechanik jetzt an einem **lokalen** [`NormalSftpChannel`]
    /// vor: Der Zwilling belegt damit weiter, dass `Default`, `replace`,
    /// `swap` und `take` für diesen Typ sehr wohl übersetzen, und der
    /// verbotene Fall kann nur noch daran scheitern, dass `Session::sftp` ein
    /// privates Feld ist. Ein Zwilling, der bloß `lock_sftp()` aufruft, hätte
    /// das nicht mehr gezeigt: fiele eines Tages `#[derive(Default)]` an
    /// `NormalSftpChannel` weg, bestünden die Fälle still aus dem falschen
    /// Grund (spec-reviewer, Runde 1).
    ///
    /// Zuweisung ans Feld — verboten:
    /// ```compile_fail
    /// # use app_logic::session::{NormalSftpChannel, Session};
    /// fn f(session: &mut Session, mut local: NormalSftpChannel) {
    ///     session.sftp = Default::default();
    /// }
    /// ```
    /// Zwilling:
    /// ```
    /// # use app_logic::session::{NormalSftpChannel, Session};
    /// fn f(session: &mut Session, mut local: NormalSftpChannel) {
    ///     local = Default::default();
    /// }
    /// ```
    ///
    /// `std::mem::replace` am Feld — verboten:
    /// ```compile_fail
    /// # use app_logic::session::{NormalSftpChannel, Session};
    /// fn f(session: &mut Session, mut local: NormalSftpChannel) {
    ///     let _old = std::mem::replace(&mut session.sftp, Default::default());
    /// }
    /// ```
    /// Zwilling:
    /// ```
    /// # use app_logic::session::{NormalSftpChannel, Session};
    /// fn f(session: &mut Session, mut local: NormalSftpChannel) {
    ///     let _old = std::mem::replace(&mut local, Default::default());
    /// }
    /// ```
    ///
    /// `std::mem::swap` zwischen zwei Sitzungen — verboten:
    /// ```compile_fail
    /// # use app_logic::session::{NormalSftpChannel, Session};
    /// fn f(a: &mut Session, b: &mut Session, x: &mut NormalSftpChannel, y: &mut NormalSftpChannel) {
    ///     std::mem::swap(&mut a.sftp, &mut b.sftp);
    /// }
    /// ```
    /// Zwilling:
    /// ```
    /// # use app_logic::session::{NormalSftpChannel, Session};
    /// fn f(a: &mut Session, b: &mut Session, x: &mut NormalSftpChannel, y: &mut NormalSftpChannel) {
    ///     std::mem::swap(x, y);
    /// }
    /// ```
    ///
    /// `std::mem::take` am Feld — verboten:
    /// ```compile_fail
    /// # use app_logic::session::{NormalSftpChannel, Session};
    /// fn f(session: &mut Session, local: &mut NormalSftpChannel) {
    ///     let _taken = std::mem::take(&mut session.sftp);
    /// }
    /// ```
    /// Zwilling:
    /// ```
    /// # use app_logic::session::{NormalSftpChannel, Session};
    /// fn f(session: &mut Session, local: &mut NormalSftpChannel) {
    ///     let _taken = std::mem::take(local);
    /// }
    /// ```
    ///
    /// Und über die **erhaltene Referenz**: `&mut dyn SftpSession` ist nicht
    /// `Sized`, `mem::swap` scheitert schon an der Signatur — verboten:
    /// ```compile_fail
    /// # use app_logic::session::NormalSftpGuard;
    /// fn f(a: &mut NormalSftpGuard<'_>, b: &mut NormalSftpGuard<'_>) {
    ///     std::mem::swap(a.sftp().unwrap(), b.sftp().unwrap());
    /// }
    /// ```
    /// Zwilling (benutzt beide Kanäle, tauscht sie nur nicht):
    /// ```
    /// # use app_logic::session::NormalSftpGuard;
    /// async fn f(a: &mut NormalSftpGuard<'_>, b: &mut NormalSftpGuard<'_>) {
    ///     let _ = a.sftp().unwrap().stat("/x").await;
    ///     let _ = b.sftp().unwrap().stat("/x").await;
    /// }
    /// ```
    pub async fn lock_sftp(&self) -> NormalSftpGuard<'_> {
        NormalSftpGuard(self.sftp.inner.lock().await)
    }

    // Spec 0086, A3.3 / ADR 0080 (decision 4): Test-Setter, je einer pro
    // Feld, das Tests tatsächlich überschreiben. Alle hinter dem Feature
    // `test-support` wie [`Session::set_sftp_for_tests`]: Produktivbauten
    // aktivieren es nicht, also gibt es für Produktivcode keinen Weg, ein
    // Feld einer bestehenden Sitzung zu überschreiben (`cargo build
    // --workspace` lehnt einen Aufruf ab; `clippy --all-targets`/`cargo
    // test` haben das Feature an). Jeder Setter ändert genau ein Feld —
    // `set_filter_engine_for_tests`, `set_ai_provider_for_tests` und
    // `set_sudo_password_for_tests` sind die sicherheitsrelevanten.

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_chat_content_for_tests(
        &mut self,
        value: Arc<crate::chat_content_extension::ChatContentRegistry>,
    ) {
        self.parts.chat_content = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_outgoing_transform_for_tests(
        &mut self,
        value: Option<Arc<dyn ssh_manager_core::outgoing::OutgoingTransform>>,
    ) {
        self.parts.outgoing_transform = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_filter_engine_for_tests(&mut self, value: Box<dyn CommandEvaluator>) {
        self.parts.filter_engine = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_model_context_window_tokens_for_tests(&mut self, value: usize) {
        self.parts.model_context_window_tokens = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_ai_provider_for_tests(&mut self, value: Box<dyn AiProvider>) {
        self.parts.ai_provider = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_post_ingest_policy_for_tests(
        &mut self,
        value: ssh_manager_core::profiles::PostIngestPolicy,
    ) {
        self.parts.post_ingest_policy = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_sudo_password_for_tests(&mut self, value: Option<secrecy::SecretString>) {
        self.parts.sudo_password = SwappableSudoPassword::new(value);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_injection_check_provider_for_tests(&mut self, value: Option<Box<dyn AiProvider>>) {
        self.parts.injection_check_provider = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_injection_check_budget_for_tests(
        &mut self,
        value: Option<Arc<ai_providers::ProviderBudgetGuard>>,
    ) {
        self.parts.injection_check_budget = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_server_id_for_tests(&mut self, value: ServerId) {
        self.parts.server_id = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_red_risk_always_confirm_for_tests(&mut self, value: bool) {
        self.parts.red_risk_always_confirm = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_chat_session_id_for_tests(&mut self, value: AsyncMutex<Option<uuid::Uuid>>) {
        self.parts.chat_session_id = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_system_context_parts_for_tests(
        &mut self,
        value: AsyncMutex<crate::compaction::SystemContextParts>,
    ) {
        self.parts.system_context_parts = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_risk_second_opinion_provider_for_tests(
        &mut self,
        value: Option<Box<dyn AiProvider>>,
    ) {
        self.parts.risk_second_opinion_provider = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_risk_second_opinion_budget_for_tests(
        &mut self,
        value: Option<Arc<ai_providers::ProviderBudgetGuard>>,
    ) {
        self.parts.risk_second_opinion_budget = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_chat_session_store_for_tests(
        &mut self,
        value: Option<persistence_sqlite::SqliteChatSessionStore>,
    ) {
        self.parts.chat_session_store = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_ledger_store_for_tests(
        &mut self,
        value: Option<persistence_sqlite::SqliteLedgerStore>,
    ) {
        self.parts.ledger_store = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_transport_for_tests(&mut self, value: SessionTransport) {
        self.parts.transport = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_ai_provider_budget_for_tests(
        &mut self,
        value: Arc<ai_providers::ProviderBudgetGuard>,
    ) {
        self.parts.ai_provider_budget = value;
    }

    /// Spec 0085, A3.3: Setzt einen Test-Kanal ein. Hinter dem Feature
    /// `test-support` (Spec 0084, A5) statt hinter reinem `#[cfg(test)]`,
    /// weil `app-shell`s Tests ihn ebenfalls brauchen; Produktivbauten
    /// aktivieren das Feature nicht.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn set_sftp_for_tests(&self, sftp: Box<dyn SftpSession>) {
        *self.sftp.inner.lock().await = Some(sftp);
    }

    /// Spec 0066, §2: entnimmt alle eingereihten Nutzer-Nachrichten.
    pub(crate) fn take_queued_user_messages(&self) -> Vec<String> {
        std::mem::take(&mut self.chat_turn.lock().unwrap().queued)
    }

    /// Spec 0066, §1: setzt das Stopp-Flag und weckt eine gerade laufende
    /// Runde auf (bricht deren KI-Stream ab).
    pub fn request_auto_continue_stop(&self) {
        self.auto_continue_stop
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.auto_continue_stop_notify.notify_waiters();
    }

    /// Spec 0017, Abschnitt 5 / Spec 0104, Abschnitt 5 (Issue #66): Lehnt
    /// eine noch wartende Bestätigung dieser Sitzung ab (fail closed). Gilt
    /// für **jede** Sitzung, ohne Unterscheidung nach Herkunft — die Aktion
    /// wird über denselben Kanal abgelehnt wie per Klick auf "Ablehnen", die
    /// wartende Ausführungskette verbucht sie also genau wie eine
    /// Nutzer-Ablehnung. Kann nie genehmigen.
    ///
    /// Ist nichts mehr offen (keine Aktion, oder schon per Klick, Timeout
    /// oder Frontend-Ablehnung aufgelöst), ist der Aufruf ein No-op ohne
    /// Fehler. Gibt zurück, ob dieser Aufruf die Bestätigung abgelehnt hat.
    pub fn reject_pending_confirmation(
        &self,
        confirmations: &ConfirmationRegistry<ActionId, crate::dto::ActionUserDecision>,
    ) -> bool {
        let Some(action_id) = *crate::poison::lock_tolerating_poison(&self.pending_action) else {
            return false;
        };
        // Kann nur scheitern, wenn die Bestätigung im selben Moment
        // anderweitig aufgelöst wurde (Klick, Timeout, Frontend-Ablehnung)
        // — dann gibt es nichts mehr abzulehnen.
        match confirmations.resolve(&action_id, crate::dto::ActionUserDecision::Deny) {
            Ok(()) => true,
            Err(err) => {
                tracing::debug!(error = %err, "pending confirmation already settled on close");
                false
            }
        }
    }

    /// Wird fertig, sobald `auto_continue_stop` gesetzt ist.
    pub(crate) async fn auto_continue_stop_requested(&self) {
        loop {
            // `notified()` vor der Flag-Prüfung anlegen: ein `Notified`
            // empfängt `notify_waiters()` ab seiner Erzeugung, auch
            // ungepollt — sonst ginge ein Stopp zwischen Prüfung und
            // `.await` verloren.
            let notified = self.auto_continue_stop_notify.notified();
            if self
                .auto_continue_stop
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return;
            }
            notified.await;
        }
    }
}

/// Issue #66: Der Tauri-freie Teil von `disconnect` für **jede** Sitzung,
/// aufgerufen, nachdem die Sitzung aus dem `SessionManager` genommen wurde
/// und bevor ihr Transport getrennt wird. `session` ist `None`, wenn sie
/// dort nicht (mehr) stand (z. B. Schließen während des Host-Key-Dialogs).
///
/// Reihenfolge ist Absicht: Erst die MCP-Buchhaltung
/// ([`McpSessionRegistry::end_session`](crate::mcp_sessions::McpSessionRegistry::end_session)
/// — Austragen, und bei MCP-Sitzungen schon dort das Ablehnen), danach das
/// sitzungsunabhängige Ablehnen. So ist eine MCP-Sitzung bereits
/// ausgetragen, wenn ihre wartende Aktion aufwacht, und der MCP-Client
/// bekommt weiter "Sitzung geschlossen" statt "vom Nutzer abgelehnt"
/// (Spec 0104, Abschnitt 5). Für eine MCP-Sitzung ist der zweite Schritt
/// dann ein No-op; für eine Nutzer-Sitzung ist er der einzige.
pub fn reject_pending_confirmation_on_close(
    session_id: SessionId,
    session: Option<&Session>,
    mcp_sessions: &crate::mcp_sessions::McpSessionRegistry,
    confirmations: &ConfirmationRegistry<ActionId, crate::dto::ActionUserDecision>,
) {
    mcp_sessions.end_session(session_id, session, confirmations);
    if let Some(session) = session {
        session.reject_pending_confirmation(confirmations);
    }
}

/// Spec 0039, Abschnitt 5: "Bei Session Resume mit vorbelasteter Historie
/// startet die Sitzung mit `true`." Es gibt aktuell **keinen** Resume-Pfad
/// (`docs/specs/0034-chat-session-persistence.md` ist noch Entwurf,
/// `SessionContext.history` startet in `app_shell::commands::connect` immer
/// mit `Vec::new()`) — diese Funktion ist die dafür vorbereitete Prüfung,
/// heute aber faktisch immer mit einer leeren Historie aufgerufen.
///
/// `MessageContent::CommandResult` zählt IMMER als untrusted-Inhalt: das
/// darin gehaltene `CommandOutput` wird erst beim tatsächlichen Versand an
/// den KI-Provider über `ai::fence_untrusted` in `<stdout>`/`<stderr>`-Tags
/// gepackt (`ai-providers::{anthropic,openai_compatible}::
/// format_command_result`) — im in-memory `ChatMessage` selbst steht noch
/// kein Fence-Text, ein reiner String-Scan würde solche Einträge sonst
/// übersehen. `MessageContent::Text` (SFTP-Dateiinhalt, ggf. künftig
/// weitere Fälle) wird dagegen bereits VOR dem Push in die Historie
/// gefenced (s. `orchestration::execute_read_remote_file`), erkennbar am
/// literalen Tag-Text.
pub fn history_contains_untrusted_content(history: &[ChatMessage]) -> bool {
    // Spec 0064 Nachtrag: aus `fence_markers()` abgeleitet statt einer
    // separat hier gepflegten Tag-Liste — ein hartcodiertes Array hätte
    // bei der Einführung von `UntrustedKind::RemoteOsInfo` in diesem
    // Schritt still unvollständig bleiben können (genau die Drift-Gefahr,
    // vor der `fence_markers()`s eigener Doc-Kommentar warnt). Filtert auf
    // öffnende Tags (kein `</...>`, kein gemeinsames `<source>`).
    let open_tags: Vec<String> = ssh_manager_core::ai::fence_markers()
        .into_iter()
        .filter(|marker| {
            marker.starts_with('<') && !marker.starts_with("</") && marker != "<source>"
        })
        .collect();
    history.iter().any(|message| match &message.content {
        MessageContent::CommandResult { .. } => true,
        MessageContent::Text(text) => open_tags.iter().any(|tag| text.contains(tag.as_str())),
        // Kommando/Grund stammen von der KI selbst bzw. der lokalen
        // Filter-Engine, nie vom Remote-Server (s. `format_action_
        // rejected`-Doc-Kommentar in `ai-providers`) — keine untrusted
        // Quelle.
        MessageContent::ActionRejected { .. } => false,
        // Issue #162: Inhalt aus dem offenen Web (Spec 0039).
        MessageContent::WebActivity(_) => true,
        // Issue #504 (ADR 0141, decision 4): every extension part, also
        // after resume and whatever its source.
        MessageContent::Extension(_) => true,
        MessageContent::Sealed(_) => true,
        // Issue #545: AI-originated text, like an assistant `Text`.
        MessageContent::Question { .. } => false,
    })
}

/// Startet den Terminal-Aktor (Modul-Kommentar, Punkt 3) als eigenen Task.
/// Läuft, bis `shell.read()` EOF liefert (leerer Chunk, s.
/// `ssh_transport::RusshShell::read`), ein Lesefehler auftritt, oder der
/// Sender-Teil des Kommando-Kanals gedroppt wird (Session verworfen).
pub fn spawn_terminal_actor(
    session_id: SessionId,
    session: Arc<Session>,
    mut shell: Box<dyn ssh_manager_core::ssh::InteractiveShell>,
    mut commands: mpsc::UnboundedReceiver<TerminalCommand>,
    emitter: Arc<dyn EventEmitter>,
) {
    tokio::spawn(async move {
        // `Some(reason)` löst am Ende ein `connection-status-changed`-Event
        // aus, `None` unterdrückt es bewusst — das passiert nur, wenn der
        // Sender absichtlich gedroppt wurde (`app_shell::commands::disconnect`
        // hat `session.terminal` auf `None` gesetzt), und dieser Befehl hat
        // das Event dafür bereits selbst gesendet. Ohne diese Unterscheidung
        // gäbe es bei jedem expliziten `disconnect()` zwei Events statt
        // eines (einmal vom Befehl, einmal vom hier endenden Aktor).
        let disconnect_reason: Option<Option<String>> = loop {
            tokio::select! {
                read_result = shell.read() => {
                    match read_result {
                        Ok(data) if !data.is_empty() => {
                            crate::events::emit_terminal_output(emitter.as_ref(), session_id, &data);
                        }
                        Ok(_) => break Some(None), // leerer Chunk == EOF/Close (s. InteractiveShell-Doc)
                        Err(err) => break Some(Some(err.to_string())),
                    }
                }
                cmd = commands.recv() => {
                    match cmd {
                        Some(TerminalCommand::Write(data)) => {
                            if let Err(err) = shell.write(&data).await {
                                break Some(Some(err.to_string()));
                            }
                        }
                        Some(TerminalCommand::Resize(size)) => {
                            // Ein Resize-Fehler ist nicht fatal für die
                            // laufende Sitzung (z. B. kurzzeitiger
                            // Channel-Hänger) — anders als ein Lese-/
                            // Schreibfehler kein Grund, den Aktor zu
                            // beenden.
                            if let Err(err) = shell.resize(size).await {
                                eprintln!("terminal_resize fehlgeschlagen: {err}");
                            }
                        }
                        None => break None, // absichtlich beendet, s. o.
                    }
                }
            }
        };

        if let Some(reason) = disconnect_reason {
            // Spec 0017, Abschnitt 2: unerwarteter Verbindungsabbruch (nicht
            // über den expliziten `disconnect()`-Befehl) muss sich auch in
            // `list_sessions()` widerspiegeln, solange der Tab im Frontend
            // noch offen ist — sonst zeigt eine wiederhergestellte Tab-Leiste
            // (Abschnitt 2, "Backend ist maßgebliche Quelle") eine tote
            // Session fälschlich als weiterhin verbunden an.
            report_connection_lost(session_id, &session, emitter.as_ref(), reason);
        }
    });
}

/// Issue #353: the one status transition for every way a connection can end
/// unexpectedly (terminal EOF, keepalive timeout, any other loss the
/// transport reports). Sets `Disconnected` and emits
/// `connection-status-changed` — exactly once per session: later reports
/// find the status already `Disconnected` and do nothing. Returns whether
/// this call made the transition.
pub fn report_connection_lost(
    session_id: SessionId,
    session: &Session,
    emitter: &dyn EventEmitter,
    reason: Option<String>,
) -> bool {
    {
        let mut status = session.status.lock().unwrap();
        if *status == ConnectionStatus::Disconnected {
            return false;
        }
        *status = ConnectionStatus::Disconnected;
    }
    // Issue #356: remember that an AI turn was cut off by the loss (running
    // command, waiting confirmation or a pending follow-up round), so the
    // AI can continue after an automatic reconnect.
    let turn_active = crate::poison::lock_tolerating_poison(&session.chat_turn).running;
    session.auto_reconnect.set_ai_turn_interrupted(turn_active);
    // The hook runs first: it announces the automatic attempts before the
    // status event, so the UI never shows "lost" for a moment.
    session.auto_reconnect.fire_hook(session_id);
    emit_connection_status_changed(emitter, session_id, ConnectionStatus::Disconnected, reason);
    true
}

/// Issue #353: watches the transport's loss signal for a session, also when
/// no terminal is open. An explicit `disconnect()` marks the transport
/// closed first, so the resulting signal is not reported as a loss. Holds
/// only a weak reference: it never keeps a removed session alive.
pub fn spawn_connection_watch(
    session_id: SessionId,
    session: &Arc<Session>,
    mut watch: ssh_manager_core::ssh::ConnectionLossWatch,
    emitter: Arc<dyn EventEmitter>,
) {
    let epoch = session.transport.epoch();
    let session = Arc::downgrade(session);
    tokio::spawn(async move {
        let loss = loop {
            if let Some(loss) = watch.borrow_and_update().clone() {
                break loss;
            }
            if watch.changed().await.is_err() {
                return; // transport dropped without a loss
            }
        };
        let Some(session) = session.upgrade() else {
            return;
        };
        // Issue #565: a replaced transport's signal is not this connection's.
        if session.transport.is_closed() || session.transport.epoch() != epoch {
            return;
        }
        report_connection_lost(session_id, &session, emitter.as_ref(), loss.reason);
    });
}

/// Eine Zeile der `SessionManager::snapshot()`-Momentaufnahme (Spec 0017,
/// Abschnitt 2) — bewusst ohne `server_name`: dafür bräuchte es den
/// `ProfileStore`, den `SessionManager` absichtlich nicht kennt (reines
/// Session-Bookkeeping, keine Persistenz-Abhängigkeit). Die Auflösung auf
/// `SessionSummaryDto` (inkl. Servername) übernimmt der Aufrufer
/// (`app_shell::commands::list_sessions`).
#[derive(Debug, Clone, Copy)]
pub struct SessionSnapshotEntry {
    pub session_id: SessionId,
    pub server_id: ServerId,
    pub status: ConnectionStatus,
    pub has_pending_action: bool,
}

/// Kapselt `AppState.sessions` (Aufgabenstellung Teil 2, Punkt 1) — s.
/// Modul-Kommentar Punkt 1 zur Wahl von `std::sync::Mutex` für die äußere
/// Map.
#[derive(Default)]
pub struct SessionManager {
    sessions: StdMutex<HashMap<SessionId, Arc<Session>>>,
    /// Spec 0017, Abschnitt 2: Verbindungsversuche, die aktuell auf
    /// `confirm_host_key` warten (`app_shell::commands::connect`) — diese
    /// Sessions existieren noch nicht in `sessions` (s. dortiger
    /// Kommentar zur Reihenfolge: `Session` wird erst nach erfolgreichem
    /// Aufbau eingefügt), sollen aber trotzdem in `list_sessions()`
    /// auftauchen (Status `AwaitingHostKey`), damit ein Frontend-Reload
    /// während eines offenen Host-Key-Dialogs den Tab nicht einfach
    /// verschwinden lässt.
    pending_connections: StdMutex<HashMap<SessionId, ServerId>>,
    /// Issue #242: Chat-Sitzungen, die gerade fortgesetzt (Resume) oder
    /// gelöscht werden. Schließt die Lücke zwischen "Resume gestartet" und
    /// `insert` der registrierten `Session`, in der `is_chat_session_active`
    /// noch `false` liefert. Reine synchrone Sperre, nie über ein `.await`
    /// hinweg gehalten.
    claimed_chat_sessions: StdMutex<HashMap<uuid::Uuid, ChatSessionClaimState>>,
}

#[derive(Default, Clone, Copy)]
struct ChatSessionClaimState {
    resumers: usize,
    deleting: bool,
}

/// Die Chat-Sitzung ist gerade in Benutzung (Resume läuft, ein Tab nutzt
/// sie) bzw. wird gerade gelöscht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatSessionClaimError {
    /// Ein Tab nutzt die Sitzung oder ein Resume läuft gerade.
    InUse,
    /// Ein Löschvorgang für diese Sitzung läuft gerade.
    BeingDeleted,
}

/// RAII-Anspruch auf eine Chat-Sitzung; gibt sie beim `Drop` frei — auch auf
/// jedem Fehlerpfad eines Resume.
#[must_use = "dropping the claim releases it immediately"]
pub struct ChatSessionClaim<'a> {
    manager: &'a SessionManager,
    chat_session_id: uuid::Uuid,
    exclusive: bool,
}

impl Drop for ChatSessionClaim<'_> {
    fn drop(&mut self) {
        let mut claims = self.manager.claimed_chat_sessions.lock().unwrap();
        if let Some(state) = claims.get_mut(&self.chat_session_id) {
            if self.exclusive {
                state.deleting = false;
            } else {
                state.resumers = state.resumers.saturating_sub(1);
            }
            if !state.deleting && state.resumers == 0 {
                claims.remove(&self.chat_session_id);
            }
        }
    }
}

impl SessionManager {
    /// Issue #242: Anspruch eines Resume. Mehrere gleichzeitige Resumes
    /// derselben Sitzung bleiben möglich (wie bisher); nur ein laufendes
    /// Löschen schließt sie aus. Vor dem Laden der Historie nehmen, bis nach
    /// `insert` der `Session` halten.
    pub fn claim_chat_session_for_resume(
        &self,
        chat_session_id: uuid::Uuid,
    ) -> Result<ChatSessionClaim<'_>, ChatSessionClaimError> {
        let mut claims = self.claimed_chat_sessions.lock().unwrap();
        let state = claims.entry(chat_session_id).or_default();
        if state.deleting {
            return Err(ChatSessionClaimError::BeingDeleted);
        }
        state.resumers += 1;
        Ok(ChatSessionClaim {
            manager: self,
            chat_session_id,
            exclusive: false,
        })
    }

    /// Issue #242: exklusiver Anspruch für das Löschen. Scheitert mit
    /// `InUse`, wenn ein Resume läuft oder ein Tab die Sitzung nutzt;
    /// Prüfung und Anspruch gehören zusammen, der Aufrufer löscht, solange
    /// er die Rückgabe hält.
    pub async fn claim_chat_session_for_delete(
        &self,
        chat_session_id: uuid::Uuid,
    ) -> Result<ChatSessionClaim<'_>, ChatSessionClaimError> {
        {
            let mut claims = self.claimed_chat_sessions.lock().unwrap();
            let state = claims.entry(chat_session_id).or_default();
            if state.deleting {
                return Err(ChatSessionClaimError::BeingDeleted);
            }
            if state.resumers > 0 {
                return Err(ChatSessionClaimError::InUse);
            }
            state.deleting = true;
        }
        // Ab hier hält der Anspruch (Drop gibt frei, auch beim Fehler unten).
        let claim = ChatSessionClaim {
            manager: self,
            chat_session_id,
            exclusive: true,
        };
        if self.is_chat_session_active(chat_session_id).await {
            return Err(ChatSessionClaimError::InUse);
        }
        Ok(claim)
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, id: SessionId, session: Arc<Session>) {
        self.sessions.lock().unwrap().insert(id, session);
    }

    pub fn get(&self, id: SessionId) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().get(&id).cloned()
    }

    pub fn remove(&self, id: SessionId) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().remove(&id)
    }

    /// Spec 0040, Abschnitt 7: Grundlage für `commands::delete_chat_session`s
    /// Schutz vor dem Löschen einer gerade aktiv verbundenen Sitzung — ohne
    /// diese Prüfung würde die zugehörige `chat_sessions`-Zeile unter einer
    /// noch laufenden `Session` weggezogen, und jeder weitere `push_history`-
    /// Aufruf dieser Sitzung liefe fortan ins Leere (Fremdschlüssel-Verletzung
    /// bei jedem Schreibversuch, nur als Warnung geloggt statt sichtbar zu
    /// scheitern — s. `orchestration::push_history_scoped`). Klont nur die
    /// `Arc`-Zeiger unter der kurzen synchronen Sperre, bevor der `.await`
    /// auf `chat_session_id` je Session erfolgt — derselbe Grund wie bei
    /// `snapshot()` oben.
    pub async fn is_chat_session_active(&self, chat_session_id: uuid::Uuid) -> bool {
        let sessions: Vec<Arc<Session>> = self.sessions.lock().unwrap().values().cloned().collect();
        for session in sessions {
            if *session.chat_session_id.lock().await == Some(chat_session_id) {
                return true;
            }
        }
        false
    }

    pub fn register_pending_connection(&self, id: SessionId, server_id: ServerId) {
        self.pending_connections
            .lock()
            .unwrap()
            .insert(id, server_id);
    }

    pub fn clear_pending_connection(&self, id: SessionId) {
        self.pending_connections.lock().unwrap().remove(&id);
    }

    /// Momentaufnahme aller offenen Sessions plus aller noch auf einen
    /// Host-Key wartenden Verbindungsversuche (Spec 0017, Abschnitt 2) —
    /// Grundlage für `list_sessions()`. Jeder Feldzugriff (`status`,
    /// `pending_action`) sperrt nur den jeweils eigenen `Session`-internen
    /// Mutex kurz, nie die äußere `sessions`-Map über den ganzen Aufbau
    /// dieser Liste hinweg — bei vielen gleichzeitig offenen Sessions blockt
    /// das keinen parallelen `insert`/`get`/`remove`-Aufruf für nennenswerte
    /// Zeit.
    pub fn snapshot(&self) -> Vec<SessionSnapshotEntry> {
        let mut result: Vec<SessionSnapshotEntry> = self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .map(|(id, session)| SessionSnapshotEntry {
                session_id: *id,
                server_id: session.server_id,
                status: *session.status.lock().unwrap(),
                // Spec 0088, A2.1: Der Tab-Indikator darf nicht daran
                // scheitern, dass irgendwo unter dieser Sperre einmal ein
                // Panic lief — sonst reißt eine einzelne vergiftete
                // Sitzung die Sitzungsliste aller Tabs mit.
                has_pending_action: crate::poison::lock_tolerating_poison(&session.pending_action)
                    .is_some(),
            })
            .collect();

        result.extend(
            self.pending_connections
                .lock()
                .unwrap()
                .iter()
                .map(|(id, server_id)| SessionSnapshotEntry {
                    session_id: *id,
                    server_id: *server_id,
                    status: ConnectionStatus::AwaitingHostKey,
                    has_pending_action: false,
                }),
        );

        result
    }
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use futures::Stream;
    use uuid::Uuid;

    use ssh_manager_core::ai::{
        default_action_schemas, AiEvent, AiProvider, DefaultOutputRedactor, RejectionReason, Role,
        SessionContext,
    };
    use ssh_manager_core::ssh::{CommandOutput, InteractiveShell, SshError};

    use super::*;
    use crate::policy::NoRulesPolicyStore;

    /// Issue #464: a secret value added after the session was built is masked
    /// in output and text, survives a swap of the inner redactor, and an empty
    /// value does not mask everything.
    #[test]
    fn test_added_secret_values_are_masked_and_survive_a_swap() {
        let redactor = SwappableRedactor::new(Box::new(DefaultOutputRedactor::new()));
        let text = "value=plain-secret-42 end";
        assert_eq!(
            redactor.redact_text(text),
            text,
            "unknown before it is added"
        );

        redactor.add_secret_values(&[
            secrecy::SecretString::from("plain-secret-42".to_string()),
            secrecy::SecretString::from(String::new()),
        ]);
        assert_eq!(redactor.redact_text(text), "value=[REDACTED] end");
        let output = CommandOutput {
            stdout: b"a plain-secret-42 b".to_vec(),
            stderr: b"plain-secret-42".to_vec(),
            exit_code: Some(0),
            truncated: false,
        };
        let redacted = redactor.redact(&output);
        assert_eq!(redacted.stdout, b"a [REDACTED] b".to_vec());
        assert_eq!(redacted.stderr, b"[REDACTED]".to_vec());
        assert_eq!(redacted.exit_code, Some(0));

        redactor.replace(Box::new(DefaultOutputRedactor::new()));
        assert_eq!(redactor.redact_text(text), "value=[REDACTED] end");
    }

    /// Wird in diesen Tests nie tatsächlich aufgerufen (nur die
    /// `SessionManager`-Buchführung selbst steht hier auf dem Prüfstand,
    /// nicht der Chat-/Terminal-Ablauf, s. `crate::orchestration`s Tests
    /// dafür) — `unreachable!` statt einer stillen Fake-Antwort macht einen
    /// versehentlichen Aufruf sofort sichtbar.
    struct UnusedAiProvider;
    impl AiProvider for UnusedAiProvider {
        fn send(&self, _ctx: SessionContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
            unreachable!("dieser Test ruft AiProvider::send nie auf")
        }
    }

    struct UnusedTransport;
    #[async_trait]
    impl SshTransport for UnusedTransport {
        async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
            unreachable!("dieser Test ruft SshTransport::execute nie auf")
        }
        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn InteractiveShell>, SshError> {
            unreachable!("dieser Test ruft SshTransport::open_shell nie auf")
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            Ok(())
        }
    }

    fn dummy_session(server_id: ServerId) -> Session {
        Session::new(SessionParts {
            transport: SessionTransport::new(Box::new(UnusedTransport)),
            ai_provider: Box::new(UnusedAiProvider),
            ai_provider_budget: Arc::new(ai_providers::ProviderBudgetGuard::new()),
            context: AsyncMutex::new(SessionContext {
                system_context: String::new(),
                history: Vec::new(),
                available_actions: default_action_schemas(),
                max_tokens_hint: None,
            }),
            filter_engine: Box::new(FilterEngine::new(NoRulesPolicyStore)),
            server_id,
            tags: Vec::new(),
            terminal: StdMutex::new(None),
            redactor: SwappableRedactor::new(Box::new(DefaultOutputRedactor::new())),
            ai_provider_label: "test-provider".to_string(),
            ai_model: "test-model".to_string(),
            system_context_parts: AsyncMutex::new(crate::compaction::SystemContextParts::default()),
            // Absichtlich riesig: Tests sollen nie versehentlich in die
            // Etappe-2-Kompaktierung laufen, außer sie setzen diesen Wert
            // explizit klein (s. `orchestration::tests`, `compaction`-Modul
            // für die dedizierten Kompaktierungs-Tests).
            model_context_window_tokens: usize::MAX / 1_000,
            summary: AsyncMutex::new(None),
            context_usage: Default::default(),
            mcp_origin_flags: StdMutex::new(Vec::new()),
            sudo_password: SwappableSudoPassword::new(None),
            status: StdMutex::new(ConnectionStatus::Connected),
            pending_action: StdMutex::new(None),
            mcp_confirmation_claim: std::sync::atomic::AtomicBool::new(false),
            auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
            auto_continue_stop_notify: tokio::sync::Notify::new(),
            chat_turn: std::sync::Mutex::new(crate::session::ChatTurnState::default()),
            risk_second_opinion_provider: None,
            risk_second_opinion_budget: None,
            // Spec 0092, A1.2: in Test-Fixtures wie in der App standardmäßig
            // „an".
            red_risk_always_confirm: true,
            running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
            outgoing_transform: None,
            chat_content: Arc::default(),
            untrusted_content_ingested: std::sync::atomic::AtomicBool::new(false),
            post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
            injection_check_provider: None,
            injection_check_budget: None,
            injection_suspected: std::sync::atomic::AtomicBool::new(false),
            injection_check_unavailable: std::sync::atomic::AtomicBool::new(false),
            second_opinion_setup_notice_pending: std::sync::atomic::AtomicBool::new(false),
            injection_check_inactive_notice_pending: std::sync::atomic::AtomicBool::new(false),
            chat_session_store: None,
            ledger_store: None,
            chat_session_id: AsyncMutex::new(None),
            ai_request_paced_at: AsyncMutex::new(None),
            auto_reconnect: Default::default(),
        })
    }

    /// Spec 0038, section 2a: after an extension triggers a rebuild, a
    /// newly added secret is masked by the open session of that server only.
    #[tokio::test]
    async fn test_rebuild_server_redactors_masks_new_secret_for_that_server_only() {
        use crate::session_extension::{
            rebuild_redactors_for_sessions, ServerScope, SessionExtension,
        };
        use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};
        use secrecy::SecretString;

        struct Changing(StdMutex<Vec<String>>);
        impl SessionExtension for Changing {
            fn known_secrets(&self, _scope: &ServerScope) -> Vec<SecretString> {
                self.0
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|s| SecretString::from(s.clone()))
                    .collect()
            }
        }

        let manager = SessionManager::new();
        let server_id = ServerId::new();
        let other_server = ServerId::new();
        let id = Uuid::new_v4();
        let other_id = Uuid::new_v4();
        manager.insert(id, Arc::new(dummy_session(server_id)));
        manager.insert(other_id, Arc::new(dummy_session(other_server)));
        let ext = Changing(StdMutex::new(Vec::new()));
        let profiles = InMemoryProfileStore::new();
        let credentials = InMemoryCredentialStore::new();
        let text = "key=late-secret-77";

        assert_eq!(manager.get(id).unwrap().redactor.redact_text(text), text);

        ext.0.lock().unwrap().push("late-secret-77".to_string());
        rebuild_redactors_for_sessions(&manager, &profiles, &credentials, Some(&ext), server_id)
            .await;

        assert!(!manager
            .get(id)
            .unwrap()
            .redactor
            .redact_text(text)
            .contains("late-secret-77"));
        // the session of the other server keeps its old redactor
        assert_eq!(
            manager.get(other_id).unwrap().redactor.redact_text(text),
            text
        );
    }

    /// Issue #635: `replace_with_literals` installs the new set atomically —
    /// a still-known value is never unmasked while another thread replaces
    /// the set — and drops empty values.
    #[test]
    fn test_replace_with_literals_is_atomic_for_known_values() {
        use secrecy::SecretString;
        let red = Arc::new(SwappableRedactor::new(Box::new(
            DefaultOutputRedactor::new(),
        )));
        let keep = SecretString::from("keep-secret-9".to_string());
        red.add_secret_values(std::slice::from_ref(&keep));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader = {
            let (red, stop) = (red.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut leaks = 0;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    if red
                        .redact_text("x keep-secret-9 y")
                        .contains("keep-secret-9")
                    {
                        leaks += 1;
                    }
                }
                leaks
            })
        };
        for _ in 0..100 {
            red.replace_with_literals(
                Box::new(DefaultOutputRedactor::new()),
                &[keep.clone(), SecretString::from(String::new())],
            );
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(reader.join().unwrap(), 0);
        assert_eq!(red.redact_text("a keep-secret-9"), "a [REDACTED]");
        red.replace_with_literals(Box::new(DefaultOutputRedactor::new()), &[]);
        assert_eq!(red.redact_text("a keep-secret-9"), "a keep-secret-9");
    }

    /// Issue #635: a rebuild with a known set replaces the literal values;
    /// a plain swap (reconnect) keeps them.
    #[tokio::test]
    async fn test_rebuild_replaces_literals_but_reconnect_swap_keeps_them() {
        use crate::session_extension::{
            rebuild_redactors_for_sessions, ServerScope, SessionExtension,
        };
        use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};
        use secrecy::SecretString;

        struct Known(Vec<String>);
        impl SessionExtension for Known {
            fn known_secrets(&self, _scope: &ServerScope) -> Vec<SecretString> {
                self.0.iter().cloned().map(SecretString::from).collect()
            }
        }

        let manager = SessionManager::new();
        let server_id = ServerId::new();
        let id = Uuid::new_v4();
        manager.insert(id, Arc::new(dummy_session(server_id)));
        let session = manager.get(id).unwrap();
        let text = "a=value-aaa-1 b=value-bbb-2";
        session
            .redactor
            .add_secret_values(&[SecretString::from("value-aaa-1".to_string())]);
        assert_eq!(
            session.redactor.redact_text(text),
            "a=[REDACTED] b=value-bbb-2"
        );

        // reconnect-style swap: A stays masked
        session.swap_redactor(Box::new(DefaultOutputRedactor::new()));
        assert_eq!(
            session.redactor.redact_text(text),
            "a=[REDACTED] b=value-bbb-2"
        );

        // rebuild with {B}: A no longer masked, B is
        let ext = Known(vec!["value-bbb-2".to_string()]);
        let profiles = InMemoryProfileStore::new();
        let credentials = InMemoryCredentialStore::new();
        rebuild_redactors_for_sessions(&manager, &profiles, &credentials, Some(&ext), server_id)
            .await;
        assert_eq!(
            session.redactor.redact_text(text),
            "a=value-aaa-1 b=[REDACTED]"
        );

        // a value still known stays masked after another rebuild, and a
        // later reconnect swap keeps it
        rebuild_redactors_for_sessions(&manager, &profiles, &credentials, Some(&ext), server_id)
            .await;
        session.swap_redactor(Box::new(DefaultOutputRedactor::new()));
        assert_eq!(
            session.redactor.redact_text(text),
            "a=value-aaa-1 b=[REDACTED]"
        );

        // rebuild with an empty set withdraws everything
        let none = Known(Vec::new());
        rebuild_redactors_for_sessions(&manager, &profiles, &credentials, Some(&none), server_id)
            .await;
        assert_eq!(session.redactor.redact_text(text), text);
    }

    // --- Spec 0039, Abschnitt 5: history_contains_untrusted_content -------

    #[test]
    fn test_history_contains_untrusted_content_false_for_empty_history() {
        assert!(!history_contains_untrusted_content(&[]));
    }

    #[test]
    fn test_history_contains_untrusted_content_false_for_plain_text_only() {
        let history = vec![ChatMessage {
            role: Role::User,
            content: MessageContent::Text("Wie geht es dir?".to_string()),
        }];
        assert!(!history_contains_untrusted_content(&history));
    }

    #[test]
    fn test_history_contains_untrusted_content_true_for_command_result() {
        let history = vec![ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::CommandResult {
                command: "ls -la".to_string(),
                output: CommandOutput {
                    stdout: b"total 0".to_vec(),
                    stderr: Vec::new(),
                    exit_code: Some(0),
                    truncated: false,
                },
                cancelled: false,
                waiting_for_input: false,
            },
        }];
        assert!(
            history_contains_untrusted_content(&history),
            "CommandResult wird beim Versand gefenced (ai-providers::format_command_result), \
             zählt also schon hier als untrusted-Inhalt"
        );
    }

    #[test]
    fn test_history_contains_untrusted_content_true_for_fenced_remote_file_text() {
        let history = vec![ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::Text(
                "Inhalt von '/etc/hosts':\n\n<remote_file>\n<source>/etc/hosts</source>\n\
                 127.0.0.1 localhost\n</remote_file>"
                    .to_string(),
            ),
        }];
        assert!(history_contains_untrusted_content(&history));
    }

    /// Issue #504 (ADR 0141, decision 4): a resumed history with an extension
    /// part sets the untrusted-content escalation, with no other untrusted
    /// content around.
    #[test]
    fn test_history_contains_untrusted_content_true_for_extension_part() {
        use ssh_manager_core::ai::{ExtensionMedia, ExtensionPart};
        let text = ChatMessage {
            role: Role::User,
            content: MessageContent::Text("hello".to_string()),
        };
        assert!(!history_contains_untrusted_content(std::slice::from_ref(
            &text
        )));
        let part = ChatMessage {
            role: Role::User,
            content: MessageContent::Extension(ExtensionPart {
                extension: "files".into(),
                kind: "k".into(),
                id: "i".into(),
                label: "a.txt".into(),
                media: ExtensionMedia::Text,
                estimated_tokens: 3,
            }),
        };
        assert!(history_contains_untrusted_content(&[text, part]));
    }

    /// Spec 0064 Nachtrag: eine Regression dieses Schritts selbst gefangen
    /// — der `uname`-Banner zog von `SystemContextParts` in eine eigene,
    /// gefencte Verlaufs-Nachricht (`UntrustedKind::RemoteOsInfo`,
    /// Tag `<remote_system>`) um; ein hartcodiertes `FENCE_OPEN_TAGS`-Array
    /// hier hätte diesen neuen Tag glatt vergessen können (genau das ist
    /// vor dem Fix passiert, bevor auf `fence_markers()` umgestellt wurde)
    /// — eine Sitzung, deren einziger Untrusted-Inhalt der Banner ist
    /// (kein Notiz, kein Kommando bisher), hätte
    /// `untrusted_content_ingested` dann fälschlich nie gesetzt.
    #[test]
    fn test_history_contains_untrusted_content_true_for_fenced_remote_os_info_text() {
        let history = vec![ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::Text(
                "<remote_system>\n<source>uname -a</source>\nLinux srv1 5.10.0\n</remote_system>"
                    .to_string(),
            ),
        }];
        assert!(history_contains_untrusted_content(&history));
    }

    #[test]
    fn test_history_contains_untrusted_content_false_for_action_rejected() {
        let history = vec![ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::ActionRejected {
                command: "rm -rf /".to_string(),
                reason: RejectionReason::User,
            },
        }];
        assert!(
            !history_contains_untrusted_content(&history),
            "Kommando/Grund stammen von der KI/lokalen Filter-Engine, nie vom Remote-Server"
        );
    }

    fn status_events(emitter: &crate::events::TestEmitter) -> Vec<serde_json::Value> {
        emitter
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == "connection-status-changed")
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    async fn wait_for_status_events(emitter: &crate::events::TestEmitter, count: usize) {
        for _ in 0..200 {
            if status_events(emitter).len() >= count {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    /// Issue #353: terminal EOF first, then a later loss report (exec/SFTP
    /// error, keepalive) must not emit a second `disconnected` event.
    #[test]
    fn test_connection_loss_is_reported_once_per_session() {
        let session = dummy_session(ServerId::new());
        let emitter = crate::events::TestEmitter::default();
        let id = Uuid::new_v4();

        assert!(report_connection_lost(id, &session, &emitter, None));
        assert!(!report_connection_lost(
            id,
            &session,
            &emitter,
            Some("The server stopped responding".to_string())
        ));
        assert_eq!(status_events(&emitter).len(), 1);
        assert_eq!(
            *session.status.lock().unwrap(),
            ConnectionStatus::Disconnected
        );
    }

    /// Issue #353: a chat-only session (no terminal actor) is marked
    /// `Disconnected` by the watcher, with the reason, exactly once even if
    /// the loss is reported again afterwards.
    #[tokio::test]
    async fn test_connection_watch_reports_loss_without_terminal() {
        let session = Arc::new(dummy_session(ServerId::new()));
        let emitter = Arc::new(crate::events::TestEmitter::default());
        let (tx, rx) = tokio::sync::watch::channel(None);
        let id = Uuid::new_v4();
        spawn_connection_watch(id, &session, rx, emitter.clone());

        tx.send(Some(ssh_manager_core::ssh::ConnectionLoss {
            reason: Some("The server stopped responding".to_string()),
        }))
        .unwrap();
        wait_for_status_events(&emitter, 1).await;

        let events = status_events(&emitter);
        assert_eq!(events.len(), 1);
        assert!(events[0]
            .to_string()
            .contains("The server stopped responding"));
        assert_eq!(
            *session.status.lock().unwrap(),
            ConnectionStatus::Disconnected
        );
        assert!(!report_connection_lost(
            id,
            &session,
            emitter.as_ref(),
            None
        ));
        assert_eq!(status_events(&emitter).len(), 1);
    }

    /// Issue #353: an explicit `disconnect()` closes the transport on
    /// purpose; the resulting signal is not a loss.
    #[tokio::test]
    async fn test_connection_watch_ignores_intentional_close() {
        let session = Arc::new(dummy_session(ServerId::new()));
        let emitter = Arc::new(crate::events::TestEmitter::default());
        let (tx, rx) = tokio::sync::watch::channel(None);
        spawn_connection_watch(Uuid::new_v4(), &session, rx, emitter.clone());

        session.transport.mark_closed();
        tx.send(Some(ssh_manager_core::ssh::ConnectionLoss { reason: None }))
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(status_events(&emitter).is_empty());
        assert_eq!(*session.status.lock().unwrap(), ConnectionStatus::Connected);
    }

    /// Issue #565: after a reconnect the old transport's watch stays
    /// silent, and a watch started for the new transport reports its loss.
    #[tokio::test]
    async fn test_connection_watch_after_reconnect_covers_new_transport_only() {
        let session = Arc::new(dummy_session(ServerId::new()));
        let emitter = Arc::new(crate::events::TestEmitter::default());
        let id = Uuid::new_v4();
        let (old_tx, old_rx) = tokio::sync::watch::channel(None);
        spawn_connection_watch(id, &session, old_rx, emitter.clone());

        *session.status.lock().unwrap() = ConnectionStatus::Disconnected;
        let claim = session.begin_reconnect().expect("claim");
        claim
            .succeed(
                Box::new(UnusedTransport),
                Box::new(DefaultOutputRedactor::new()),
                None,
            )
            .await;
        let (new_tx, new_rx) = tokio::sync::watch::channel(None);
        spawn_connection_watch(id, &session, new_rx, emitter.clone());

        old_tx
            .send(Some(ssh_manager_core::ssh::ConnectionLoss { reason: None }))
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(status_events(&emitter).is_empty());
        assert_eq!(*session.status.lock().unwrap(), ConnectionStatus::Connected);

        new_tx
            .send(Some(ssh_manager_core::ssh::ConnectionLoss { reason: None }))
            .unwrap();
        wait_for_status_events(&emitter, 1).await;
        assert_eq!(status_events(&emitter).len(), 1);
        assert_eq!(
            *session.status.lock().unwrap(),
            ConnectionStatus::Disconnected
        );
    }

    #[test]
    fn test_insert_get_remove_round_trip() {
        let manager = SessionManager::new();
        let id = Uuid::new_v4();
        let server_id = ServerId::new();
        manager.insert(id, Arc::new(dummy_session(server_id)));

        assert!(manager.get(id).is_some());
        assert_eq!(manager.get(id).unwrap().server_id, server_id);

        let removed = manager.remove(id);
        assert!(removed.is_some());
        assert!(manager.get(id).is_none());
    }

    #[test]
    fn test_snapshot_reports_status_and_pending_action_per_session() {
        let manager = SessionManager::new();
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let server_a = ServerId::new();
        let server_b = ServerId::new();

        let session_a = dummy_session(server_a);
        // Simuliert eine wartende Confirm-Bestätigung (Spec 0017, Abschnitt
        // 5), wie sie `orchestration::handle_action_proposed` setzt.
        *session_a.pending_action.lock().unwrap() = Some(Uuid::new_v4());
        manager.insert(id_a, Arc::new(session_a));

        let session_b = dummy_session(server_b);
        *session_b.status.lock().unwrap() = ConnectionStatus::Disconnected;
        manager.insert(id_b, Arc::new(session_b));

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.len(), 2);

        let entry_a = snapshot.iter().find(|e| e.session_id == id_a).unwrap();
        assert_eq!(entry_a.server_id, server_a);
        assert_eq!(entry_a.status, ConnectionStatus::Connected);
        assert!(
            entry_a.has_pending_action,
            "Session mit wartender Confirm-Entscheidung muss has_pending_action=true melden"
        );

        let entry_b = snapshot.iter().find(|e| e.session_id == id_b).unwrap();
        assert_eq!(entry_b.status, ConnectionStatus::Disconnected);
        assert!(!entry_b.has_pending_action);
    }

    #[test]
    fn test_snapshot_includes_pending_host_key_connections_as_awaiting() {
        let manager = SessionManager::new();
        let pending_id = Uuid::new_v4();
        let server_id = ServerId::new();
        manager.register_pending_connection(pending_id, server_id);

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].session_id, pending_id);
        assert_eq!(snapshot[0].server_id, server_id);
        assert_eq!(snapshot[0].status, ConnectionStatus::AwaitingHostKey);
        assert!(!snapshot[0].has_pending_action);

        manager.clear_pending_connection(pending_id);
        assert!(manager.snapshot().is_empty());
    }

    #[test]
    fn test_snapshot_combines_real_sessions_and_pending_connections() {
        let manager = SessionManager::new();
        let connected_id = Uuid::new_v4();
        manager.insert(connected_id, Arc::new(dummy_session(ServerId::new())));

        let pending_id = Uuid::new_v4();
        manager.register_pending_connection(pending_id, ServerId::new());

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot
            .iter()
            .any(|e| e.session_id == connected_id && e.status == ConnectionStatus::Connected));
        assert!(snapshot
            .iter()
            .any(|e| e.session_id == pending_id && e.status == ConnectionStatus::AwaitingHostKey));
    }

    // --- Spec 0040, Abschnitt 7: is_chat_session_active ---------------------

    /// Spec 0088, T15 (A3.2): Die EINE Stelle, die einen leeren normalen
    /// SFTP-Kanal in einen Fehler umwandelt, tut das an einer frischen
    /// Sitzung auch tatsächlich — statt zu panicken.
    ///
    /// Frisch heisst hier: `ensure_sftp_open` lief nie. Genau dieser Zustand
    /// stand vorher hinter sechs
    /// `expect("ensure_sftp_open lief erfolgreich durch")` in
    /// `orchestration::remote_files`.
    #[tokio::test]
    async fn test_sftp_or_err_reports_an_error_on_a_session_whose_channel_was_never_opened() {
        let session = dummy_session(ServerId::new());
        let mut guard = session.lock_sftp().await;
        assert!(!guard.is_open(), "frische Sitzung: Kanal ist zu");

        let err = guard
            .sftp_or_err()
            .err()
            .expect("ein nicht geöffneter Kanal muss ein Fehler sein, kein Erfolg");
        assert_eq!(err.code(), "SSH_CHANNEL_ERROR");
        assert!(
            guard.sftp().is_none(),
            "und `sftp()` meldet weiterhin schlicht None"
        );
    }

    #[tokio::test]
    async fn test_is_chat_session_active_true_for_a_live_session_bound_to_it() {
        let manager = SessionManager::new();
        let chat_session_id = Uuid::new_v4();
        let mut session = dummy_session(ServerId::new());
        session.set_chat_session_id_for_tests(AsyncMutex::new(Some(chat_session_id)));
        manager.insert(Uuid::new_v4(), Arc::new(session));

        assert!(manager.is_chat_session_active(chat_session_id).await);
    }

    #[tokio::test]
    async fn test_is_chat_session_active_false_for_an_unrelated_chat_session_id() {
        let manager = SessionManager::new();
        let mut session = dummy_session(ServerId::new());
        session.set_chat_session_id_for_tests(AsyncMutex::new(Some(Uuid::new_v4())));
        manager.insert(Uuid::new_v4(), Arc::new(session));

        assert!(!manager.is_chat_session_active(Uuid::new_v4()).await);
    }

    #[tokio::test]
    async fn test_is_chat_session_active_false_when_no_live_session_has_a_chat_session_id() {
        let manager = SessionManager::new();
        manager.insert(Uuid::new_v4(), Arc::new(dummy_session(ServerId::new())));

        assert!(!manager.is_chat_session_active(Uuid::new_v4()).await);
    }

    // Issue #242: Claim-Logik (Resume vs. Löschen).
    #[tokio::test]
    async fn delete_is_refused_while_a_resume_holds_the_claim() {
        let manager = SessionManager::new();
        let id = Uuid::new_v4();
        let _resume = manager.claim_chat_session_for_resume(id).unwrap();
        assert!(matches!(
            manager.claim_chat_session_for_delete(id).await,
            Err(ChatSessionClaimError::InUse)
        ));
    }

    #[tokio::test]
    async fn delete_is_allowed_again_after_a_failed_resume_drops_the_claim() {
        let manager = SessionManager::new();
        let id = Uuid::new_v4();
        let resume = manager.claim_chat_session_for_resume(id).unwrap();
        drop(resume); // Fehlerpfad: Guard fällt
        assert!(manager.claim_chat_session_for_delete(id).await.is_ok());
    }

    #[tokio::test]
    async fn delete_of_unused_chat_session_is_allowed_and_releases_afterwards() {
        let manager = SessionManager::new();
        let id = Uuid::new_v4();
        let delete = manager.claim_chat_session_for_delete(id).await.unwrap();
        // Läuft das Löschen, scheitert ein Resume und ein zweites Löschen.
        assert!(matches!(
            manager.claim_chat_session_for_resume(id),
            Err(ChatSessionClaimError::BeingDeleted)
        ));
        assert!(matches!(
            manager.claim_chat_session_for_delete(id).await,
            Err(ChatSessionClaimError::BeingDeleted)
        ));
        drop(delete);
        assert!(manager.claim_chat_session_for_resume(id).is_ok());
    }

    #[tokio::test]
    async fn claims_are_per_chat_session() {
        let manager = SessionManager::new();
        let _resume = manager
            .claim_chat_session_for_resume(Uuid::new_v4())
            .unwrap();
        assert!(manager
            .claim_chat_session_for_delete(Uuid::new_v4())
            .await
            .is_ok());
    }
}
