//! Spec 0067: erhöhter SFTP-Kanal für den Dateibrowser (Zugangsnachweis,
//! Kanalwahl, Ein-/Ausschalten) — Teil der Spec-0083-Aufteilung von
//! `commands.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tauri::State;

use ssh_manager_core::ssh::{RemoteEntry, SftpSession, SshError};

use crate::elevated_sftp::{
    ElevatedSftp, ElevatedSftpRegistry, ElevatedSftpSlot, ElevationContext,
};
use app_logic::error::{CommandError, CommandResult};
use app_logic::session::{NormalSftpGuard, Session};
use app_logic::state::{AppState, SessionId};

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser -------------------------
//
// Bewusst OHNE Filter-Engine-Prüfung — anders als `ReadRemoteFile`/
// `WriteRemoteFile` (Spec 0020, Abschnitt 4, `app_logic::orchestration`) laufen
// diese Befehle nie über den KI-Chat, sondern sind direkte Nutzeraktionen im
// Dateibrowser-Panel, analog zum interaktiven Terminal (Spec 0005, Abschnitt
// 1: auch dort läuft rohe Tastatureingabe ungefiltert durch).
//
// Historische Anmerkung (Spec 0020, Teil 1): `remove()` (SFTP `REMOVE`)
// wirkt nur auf Dateien, `sftp_download`/`sftp_delete` waren deshalb lange
// auf Dateien beschränkt. Spec 0054 hebt das auf: `sftp_download_default`/
// `sftp_download_dir` (Teil 2) laden Ordner rekursiv herunter, `sftp_delete`
// (Teil 3, unten) löscht sie rekursiv über die neuen Trait-Methoden
// `remove_dir`/das Zusammenspiel mit `list_dir`. "Umbenennen" (SFTP
// `RENAME`, für beide Eintragstypen) war davon nie betroffen.

/// Spec 0067, Teil A: Zugangsnachweis für den erhöhten SFTP-Kanal
/// (`crate::elevated_sftp::ElevatedSftpSlot::lock_owned` und jedes
/// Nachschlagen in der `ElevatedSftpRegistry`). Das private Feld macht ihn
/// außerhalb dieses Moduls unkonstruierbar — KI (`orchestration`) und MCP
/// (`mcp_backend`) kommen so nie an den erhöhten Kanal.
///
/// Spec 0084, A1: Seit der Kanal nicht mehr an der `Session` hängt, kommt
/// eine zweite Hürde dazu — wer ihn erreichen will, braucht zusätzlich die
/// Zuordnung, und die bekommt nur, wer sie als Tauri-Zustand anfordert.
pub(crate) struct BrowserAccess(());

#[cfg(test)]
impl BrowserAccess {
    /// Nur für Tests des erhöhten Kanals (`crate::elevated_sftp`).
    pub(crate) fn for_tests() -> Self {
        Self(())
    }
}

/// Welcher SFTP-Kanal eine Browser-Aktion ausführt. Das Frontend wählt ihn
/// pro Aufruf (`elevated_user`); ist der erhöhte Kanal inzwischen weg oder
/// läuft er als ein anderer Nutzer als erwartet, schlägt die Aktion fehl —
/// nie ein stiller Rückfall auf den normalen Kanal oder einen anderen
/// Nutzer (Spec 0067, A5: "kein stiller Upload als normaler Nutzer";
/// spec-reviewer-Fund: Edit-Flow darf nicht unter einem inzwischen
/// umgeschalteten Nutzer hochladen).
///
/// Spec 0084, A1: Der erhöhte Kanal hängt nicht mehr an der `Session`,
/// sondern an der [`ElevatedSftpRegistry`]. Er wird deshalb hier — beim
/// Übersetzen der Frontend-Anfrage — einmal nachgeschlagen und mitgeführt;
/// `slot: None` heißt „für diese Sitzung ist kein erhöhter Kanal aktiv“ und
/// lässt jede Aktion scheitern, nie zurückfallen.
///
/// Spec 0085, A1: Der Wert gilt für **einen** Browser-Befehl und trägt
/// dessen Abbruch-Vermerk ([`Self::revoked_mid_command`]). Er ist außerhalb
/// dieses Moduls nicht konstruierbar — der einzige Weg zu ihm führt über
/// [`run_browser_command`], und genau dort sitzt die zentrale Übersetzung des
/// Befehlsergebnisses (A1.2). Ein neuer Browser-Befehl kann die Übersetzung
/// damit nicht versehentlich weglassen.
pub(super) struct BrowserChannel {
    kind: BrowserChannelKind,
    /// Spec 0085, A1.2: `true`, sobald **irgendeine** Operation dieses
    /// Befehls wegen eines Widerrufs abgebrochen wurde. Von
    /// [`with_browser_channel`] am Ende ausgewertet und in
    /// `ELEVATED_CHANNEL_INACTIVE` übersetzt — auch dann, wenn der Befehl den
    /// SFTP-Fehler selbst abgefangen hat (`sftp_exists` → `Ok(false)`,
    /// `sftp_download`s Größen-Vorablauf → `.ok()`, `sftp_read_text`s
    /// `if let Ok`). Ohne diese Übersetzung endete ein Widerruf dort mit `Ok`
    /// und behauptete damit einen Erfolg, den es nicht gab.
    ///
    /// `Arc`, weil eine laufende Operation ihn setzen muss, während der
    /// Befehl selbst den Kanal noch hält.
    revoked_mid_command: Arc<AtomicBool>,
}

enum BrowserChannelKind {
    Normal,
    Elevated {
        /// Erwarteter Ziel-Nutzer des erhöhten Kanals.
        expected_user: String,
        slot: Option<ElevatedSftpSlot>,
    },
}

impl BrowserChannel {
    fn from_request(
        registry: &ElevatedSftpRegistry,
        session_id: SessionId,
        elevated_user: Option<String>,
    ) -> Self {
        let kind = match elevated_user {
            Some(expected_user) => BrowserChannelKind::Elevated {
                expected_user,
                slot: registry.slot(session_id, &BrowserAccess(())),
            },
            None => BrowserChannelKind::Normal,
        };
        Self {
            kind,
            revoked_mid_command: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Läuft dieser Befehl über den erhöhten Kanal? (Nicht: ob dieser noch
    /// aktiv ist.) `sftp_download` leitet daraus ab, dass eine lokale Kopie
    /// nur für den eigenen Nutzer lesbar angelegt wird.
    pub(super) fn is_elevated(&self) -> bool {
        matches!(self.kind, BrowserChannelKind::Elevated { .. })
    }

    /// Nur für Tests: Ist für diesen (erhöhten) Befehl überhaupt ein Kanal
    /// eingetragen? `false` heißt „nie eingeschaltet oder Sitzung getrennt" —
    /// dann scheitert jede Aktion, statt zurückzufallen.
    #[cfg(test)]
    fn has_elevated_slot(&self) -> bool {
        matches!(
            &self.kind,
            BrowserChannelKind::Elevated { slot: Some(_), .. }
        )
    }

    /// Nur für Tests: dieselbe Übersetzung der Frontend-Anfrage wie im
    /// Produktivpfad, aber ohne `AppState` (s. [`run_browser_command`]).
    #[cfg(test)]
    pub(super) fn for_tests(
        registry: &ElevatedSftpRegistry,
        session_id: SessionId,
        elevated_user: Option<String>,
    ) -> Self {
        Self::from_request(registry, session_id, elevated_user)
    }
}

const ELEVATED_CHANNEL_INACTIVE: &str =
    "Der erhöhte Modus ist nicht mehr aktiv (Verbindung getrennt oder ausgeschaltet) — \
     die Aktion wurde nicht ausgeführt. Bitte den erhöhten Modus erneut einschalten.";

/// Spec 0085, A1.2: **die eine** Stelle, an der das Ergebnis eines
/// Browser-Befehls in `ELEVATED_CHANNEL_INACTIVE` übersetzt wird, wenn
/// während des Befehls widerrufen wurde. Keine Regel je Befehl — der
/// Abschluss ist für alle derselbe, und ein Befehl kann ihn nicht umgehen,
/// weil er ohne diese Funktion keinen [`BrowserChannel`] bekommt.
///
/// Eigene Funktion neben [`run_browser_command`], damit Tests denselben
/// Abschluss fahren wie der Produktivpfad, ohne einen `AppState` bauen zu
/// müssen (der Unterschied ist nur das Auflösen von Sitzung und Kanal).
pub(super) async fn with_browser_channel<T, F, Fut>(
    session: Arc<Session>,
    channel: BrowserChannel,
    body: F,
) -> CommandResult<T>
where
    F: FnOnce(Arc<Session>, BrowserChannel) -> Fut,
    Fut: std::future::Future<Output = CommandResult<T>>,
{
    let revoked = channel.revoked_mid_command.clone();
    let result = body(session, channel).await;
    if revoked.load(Ordering::SeqCst) {
        // Spec 0085, A1.2: wörtlich dieselbe Meldung wie bei einem Widerruf
        // vor dem ersten Zugriff — der Aufrufer soll den Abbruchgrund nicht
        // daran unterscheiden müssen, wie weit der Befehl gekommen war.
        // Schon geänderte Einträge bleiben geändert (§2, Nicht-Ziel).
        return Err(CommandError::from(ELEVATED_CHANNEL_INACTIVE));
    }
    result
}

/// Gemeinsamer Rahmen **jedes** `sftp_*`-Browser-Befehls: Kanal aus der
/// Frontend-Anfrage auflösen, Sitzung holen (und den normalen Kanal bei
/// Bedarf öffnen), Rumpf ausführen, Ergebnis zentral abschließen.
pub(super) async fn run_browser_command<T, F, Fut>(
    state: &AppState,
    registry: &ElevatedSftpRegistry,
    session_id: SessionId,
    elevated_user: Option<String>,
    body: F,
) -> CommandResult<T>
where
    F: FnOnce(Arc<Session>, BrowserChannel) -> Fut,
    Fut: std::future::Future<Output = CommandResult<T>>,
{
    let channel = BrowserChannel::from_request(registry, session_id, elevated_user);
    let session = browser_session(state, session_id, &channel).await?;
    with_browser_channel(session, channel, body).await
}

/// Warum ein Zugriff auf den erhöhten Kanal nicht zustande kam.
enum ElevatedAccessError {
    /// Widerrufen: ausgeschaltet, Sitzung entfernt oder für einen anderen
    /// Nutzer neu aktiviert. Wird zentral in `ELEVATED_CHANNEL_INACTIVE`
    /// übersetzt (A1.2) und setzt deshalb den Abbruch-Vermerk.
    Revoked,
    /// Der Kanal läuft als ein anderer Nutzer als der, für den das Frontend
    /// diese Aktion angefragt hat (Spec 0067, A5). Kein Widerruf: die
    /// Meldung nennt beide Nutzer und bleibt deshalb erhalten.
    UserChanged { active: String, expected: String },
}

impl ElevatedAccessError {
    fn message(&self) -> String {
        match self {
            Self::Revoked => ELEVATED_CHANNEL_INACTIVE.to_string(),
            Self::UserChanged { active, expected } => format!(
                "Der erhöhte Modus läuft inzwischen als „{active}“, nicht als „{expected}“ — die \
                 Aktion wurde nicht ausgeführt."
            ),
        }
    }
}

/// Der erhöhte Kanal, gesperrt für **eine** SFTP-Operation.
///
/// Spec 0085, A1.3: Die Sperre wird je Operation genommen und danach wieder
/// freigegeben, nicht über den ganzen Befehl gehalten. Nur so wartet das
/// Ausschalten höchstens auf die gerade laufende einzelne Operation und nicht
/// auf den Rest einer Rekursion (Spec 0084 §9 spricht von einem „laufenden
/// Vorgang"; damit ist ab Spec 0085 eine einzelne SFTP-Operation gemeint).
struct ElevatedOperation(tokio::sync::OwnedMutexGuard<Option<ElevatedSftp>>);

impl ElevatedOperation {
    fn sftp(&mut self) -> &mut (dyn SftpSession + 'static) {
        &mut *self
            .0
            .as_mut()
            .expect("elevated_access hat den Kanalwert unter dieser Sperre schon geprüft")
            .sftp
    }
}

/// Spec 0084, §9 / Spec 0085, A1.1: **die maßgebliche** Prüfung zwischen einem
/// Browser-Befehl und dem erhöhten Kanal. Jeder Zugriff läuft hier durch — der
/// erste ([`BrowserSftpGuard::sftp`]) ebenso wie jeder weitere innerhalb einer
/// Rekursion ([`elevated_operation`]).
///
/// **Der Widerruf wird geprüft, nachdem die Kanal-Sperre da ist, nicht davor.**
/// Darauf kommt es an: `tokio::sync::Mutex` ist fair, ein Zugriff, der beim
/// Widerruf schon in der Warteschlange stand, käme sonst noch vor ihm an die
/// Reihe und liefe mit den alten Rechten weiter. Prüfen und Benutzen liegen so
/// in derselben kritischen Sektion, und der Kanalwert kann auch nur unter
/// dieser Sperre herausgenommen werden.
///
/// Zwei Stellen prüfen **zusätzlich** früher, und keine von beiden ersetzt
/// diese Prüfung — beide können einen Zugriff nur **ablehnen**, nie zulassen
/// (spec-reviewer, Runde 1/2):
/// * der Schnellabbruch in [`elevated_operation`], damit ein schon
///   widerrufener Zugriff nicht erst auf die Sperre wartet;
/// * [`BrowserSftpGuard::sftp`], das zu Befehlsbeginn einmal hier durchgeht
///   und das Ergebnis gleich wieder fallenlässt — dort hat die
///   `UserChanged`-Meldung ihren Platz, bevor der Befehl irgendetwas tut.
///
/// Wer hier etwas umbaut: **diese** Prüfung ist die, die trägt. „Wir haben
/// doch beim Erstzugang schon geprüft" gilt nicht — zwischen Erstzugang und
/// Operation liegt beliebig viel Zeit.
async fn elevated_access(
    slot: &ElevatedSftpSlot,
    expected_user: &str,
) -> Result<ElevatedOperation, ElevatedAccessError> {
    let guard = slot.lock_owned(&BrowserAccess(())).await;
    if slot.is_revoked() {
        return Err(ElevatedAccessError::Revoked);
    }
    match guard.as_ref() {
        // Kanalwert herausgenommen (`ElevatedSftpSlot::revoke`) — dasselbe
        // wie widerrufen, nur ist der Merker hier schon gesetzt gewesen.
        None => Err(ElevatedAccessError::Revoked),
        Some(elevated) if elevated.target_user != expected_user => {
            Err(ElevatedAccessError::UserChanged {
                active: elevated.target_user.clone(),
                expected: expected_user.to_string(),
            })
        }
        Some(_) => Ok(ElevatedOperation(guard)),
    }
}

/// Zugang für eine **einzelne** Operation eines laufenden Befehls. Setzt bei
/// einem Widerruf den Abbruch-Vermerk des Befehls, damit
/// [`with_browser_channel`] das Ergebnis danach übersetzt — auch wenn der
/// Befehl den Fehler selbst abfängt.
async fn elevated_operation(
    slot: &ElevatedSftpSlot,
    expected_user: &str,
    revoked_mid_command: &AtomicBool,
) -> Result<ElevatedOperation, SshError> {
    // Schnellabbruch: Ist schon widerrufen, wartet dieser Zugriff nicht erst
    // auf die Kanal-Sperre. Das ist keine zweite Absicherung, sondern eine
    // Abkürzung, die nur **ablehnen** kann — die Prüfung, die trägt, sitzt in
    // `elevated_access` unter der Sperre.
    //
    // Sie steht hier, weil eine einzelne Operation lange dauern kann
    // (`read_file`/`write_file` überträgt eine ganze Datei in einem Aufruf):
    // ohne sie hinge der Rest einer abgebrochenen Rekursion hinter einem
    // laufenden Transfer, statt sofort mit `ELEVATED_CHANNEL_INACTIVE`
    // zurückzukommen (spec-reviewer, Runde 2).
    if slot.is_revoked() {
        revoked_mid_command.store(true, Ordering::SeqCst);
        return Err(SshError::ChannelError(
            ElevatedAccessError::Revoked.message(),
        ));
    }
    // Haltepunkt nur für Tests, zwischen dem Schnellabbruch und dem Anfordern
    // der Sperre (s. `ElevatedSftpSlot::before_operation_hook`) — im
    // Produktivbau ein leerer Rumpf. Genau dort widerrufen die Tests, die die
    // maßgebliche Prüfung erreichen wollen: an der Abkürzung sind sie schon
    // vorbei.
    slot.run_before_operation_hook();
    elevated_access(slot, expected_user).await.map_err(|err| {
        if matches!(err, ElevatedAccessError::Revoked) {
            revoked_mid_command.store(true, Ordering::SeqCst);
        }
        SshError::ChannelError(err.message())
    })
}

/// Der für diesen Befehl gewählte SFTP-Kanal — normal oder erhöht.
///
/// Spec 0085, A1.3: Der **normale** Kanal wird wie bisher für die Dauer des
/// Befehls gesperrt (A1.5: „über den normalen Kanal ändert sich nichts"). Der
/// **erhöhte** wird hier nicht gesperrt; das passiert je Operation in
/// [`elevated_operation`].
pub(super) enum BrowserSftpGuard<'a> {
    /// Spec 0085, A3.1: `app-shell` bekommt den normalen Kanal nur noch als
    /// [`NormalSftpGuard`] — daraus lässt sich kein Kanal herausnehmen oder
    /// einsetzen, nur einer benutzen.
    Normal(NormalSftpGuard<'a>),
    Elevated {
        /// Spec 0084, §4: eine *eigene* Sperre je Sitzung, unabhängig von
        /// der Sperre der Zuordnung — ein laufender Vorgang hier hält das
        /// Trennen einer anderen Sitzung nicht auf.
        slot: ElevatedSftpSlot,
        expected_user: String,
        revoked_mid_command: Arc<AtomicBool>,
    },
    /// Spec 0084, A1/A2: Für diese Sitzung ist kein erhöhter Kanal
    /// eingetragen (nie eingeschaltet, oder die Sitzung wurde getrennt).
    /// Jede Aktion scheitert — nie ein stiller Rückfall auf den normalen
    /// Kanal (Spec 0067, A5).
    ElevatedInactive,
}

impl BrowserSftpGuard<'_> {
    /// Der Kanal, über den dieser Befehl seine SFTP-Operationen ausführt.
    ///
    /// Prüft beim **ersten** Zugriff genau das, was schon Spec 0084 §9 hier
    /// prüfte (Widerruf, herausgenommener Kanalwert, umgeschalteter
    /// Ziel-Nutzer) und scheitert wie bisher mit der jeweiligen Meldung. Die
    /// Prüfung bleibt danach nicht stehen: Der zurückgegebene Wert führt sie
    /// vor **jeder** einzelnen Operation erneut aus (Spec 0085, A1.1), auch
    /// in Befehlen, die es noch nicht gibt — ein Befehl kann keine Operation
    /// über den erhöhten Kanal ausführen, ohne durch ihn zu gehen.
    pub(super) async fn sftp(&mut self) -> CommandResult<BrowserSftp<'_>> {
        match self {
            Self::Normal(guard) => Ok(BrowserSftp::Normal(
                guard
                    .sftp()
                    .expect("browser_session öffnet den normalen Kanal vorab"),
            )),
            Self::ElevatedInactive => Err(CommandError::from(ELEVATED_CHANNEL_INACTIVE)),
            Self::Elevated {
                slot,
                expected_user,
                revoked_mid_command,
            } => {
                // Erstzugang: dieselben Prüfungen wie bisher, dieselben
                // Meldungen. Die Sperre wird sofort wieder freigegeben —
                // gehalten wird sie nur um eine einzelne Operation herum.
                match elevated_access(slot, expected_user).await {
                    Ok(_) => Ok(BrowserSftp::Elevated {
                        slot,
                        expected_user,
                        revoked_mid_command,
                    }),
                    Err(err) => {
                        if matches!(err, ElevatedAccessError::Revoked) {
                            revoked_mid_command.store(true, Ordering::SeqCst);
                        }
                        Err(CommandError::from(err.message()))
                    }
                }
            }
        }
    }

    /// Ziel-Nutzer, wenn diese Aktion über den erhöhten Kanal läuft.
    ///
    /// Spec 0085, A1.4: **Vor** den Operationen abfragen. Ein abgebrochener
    /// erhöhter `delete`/`chmod` soll seine Audit-Zeile mit dem Nutzer
    /// bekommen, unter dem die schon ausgeführten Operationen liefen — nach
    /// dem Widerruf abgefragt, läge hier `None` und die Zeile fehlte ganz.
    pub(super) fn elevated_user(&self) -> Option<String> {
        match self {
            Self::Normal(_) | Self::ElevatedInactive => None,
            // Ein widerrufener Kanal meldet keinen Ziel-Nutzer mehr: über
            // ihn läuft ohnehin keine Aktion mehr (s. `sftp`), und die
            // Audit-Zeile soll keine Erhöhung behaupten, die es nicht gab.
            Self::Elevated { slot, .. } if slot.is_revoked() => None,
            // Der Ziel-Nutzer des Kanals, nicht bloß der angefragte: `sftp()`
            // lässt keine Operation zu, solange beide auseinanderliegen
            // (`UserChanged`), beide sind hier also gleich.
            Self::Elevated { expected_user, .. } => Some(expected_user.clone()),
        }
    }
}

/// Der gewählte Kanal, bereit für SFTP-Operationen — selbst eine
/// [`SftpSession`], damit die Rekursionen (`delete_recursive`,
/// `chmod_recursive`, `walk_dirs_and_count_files`, `download_recursive`)
/// unverändert gegen `&mut dyn SftpSession` arbeiten und die Prüfung aus
/// Spec 0085 A1.1 trotzdem **jede einzelne** Operation abdeckt.
pub(super) enum BrowserSftp<'a> {
    /// Der normale Kanal, für die Dauer des Befehls gesperrt (A1.5).
    /// `&mut dyn SftpSession` statt `&mut Box<dyn SftpSession>`: eine
    /// Referenz auf das Trait-Objekt lässt sich nicht ersetzen (Spec 0085,
    /// A3.1).
    Normal(&'a mut (dyn SftpSession + 'static)),
    Elevated {
        slot: &'a ElevatedSftpSlot,
        expected_user: &'a str,
        revoked_mid_command: &'a AtomicBool,
    },
}

impl BrowserSftp<'_> {
    /// Zugang für die nächste einzelne Operation — beim erhöhten Kanal mit
    /// Widerrufsprüfung und eigener Sperre, beim normalen der schon
    /// gesperrte Kanal selbst.
    async fn operation(&mut self) -> Result<BrowserSftpOperation<'_>, SshError> {
        match self {
            Self::Normal(sftp) => Ok(BrowserSftpOperation::Normal(*sftp)),
            Self::Elevated {
                slot,
                expected_user,
                revoked_mid_command,
            } => Ok(BrowserSftpOperation::Elevated(
                elevated_operation(slot, expected_user, revoked_mid_command).await?,
            )),
        }
    }
}

enum BrowserSftpOperation<'a> {
    Normal(&'a mut (dyn SftpSession + 'static)),
    Elevated(ElevatedOperation),
}

impl BrowserSftpOperation<'_> {
    fn sftp(&mut self) -> &mut (dyn SftpSession + 'static) {
        match self {
            Self::Normal(sftp) => *sftp,
            Self::Elevated(operation) => operation.sftp(),
        }
    }
}

// Jede Trait-Methode ist derselbe Dreischritt: Zugang für diese Operation
// holen (dabei Widerruf prüfen), delegieren, Sperre freigeben. Bewusst
// ausgeschrieben statt per Makro erzeugt: `#[async_trait]` expandiert vor
// `macro_rules!`, und an dieser Stelle soll ohnehin jede einzelne Operation
// im Quelltext sichtbar sein. Eine neue Trait-Methode in `SftpSession`
// erzwingt hier einen Eintrag, sonst kompiliert die Kiste nicht.
#[async_trait]
impl SftpSession for BrowserSftp<'_> {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        self.operation().await?.sftp().list_dir(path).await
    }

    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.operation().await?.sftp().read_file(path).await
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError> {
        self.operation()
            .await?
            .sftp()
            .write_file(path, content)
            .await
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.operation().await?.sftp().stat(path).await
    }

    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.operation().await?.sftp().lstat(path).await
    }

    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        self.operation().await?.sftp().remove(path).await
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        self.operation().await?.sftp().rename(from, to).await
    }

    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.operation().await?.sftp().create_dir(path).await
    }

    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.operation().await?.sftp().remove_dir(path).await
    }

    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError> {
        self.operation()
            .await?
            .sftp()
            .set_permissions(path, mode)
            .await
    }
}

pub(super) fn write_local_download(
    path: &std::path::Path,
    bytes: &[u8],
    owner_only: bool,
) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if owner_only {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = owner_only;
    let mut file = options.open(path)?;
    std::io::Write::write_all(&mut file, bytes)?;
    #[cfg(unix)]
    if owner_only {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Spec 0067, A5 (baut auf der Audit-Erfassbarkeit aus 0054 auf): jede
/// server-verändernde Browser-Aktion im erhöhten Modus hinterlässt eine
/// strukturierte Log-Zeile mit Kennzeichnung "erhöht" + Ziel-Nutzer, Quelle
/// "manuell". Nur Aktion und Pfade, nie Dateiinhalte.
/// Auch bei einem Fehlschlag protokolliert (`ok = false`) — ein rekursives
/// Löschen/chmod kann mittendrin scheitern, nachdem schon Einträge geändert
/// wurden.
pub(super) fn audit_elevated_change(
    elevated_user: Option<&str>,
    action: &'static str,
    path: &str,
    ok: bool,
) {
    if let Some(target_user) = elevated_user {
        tracing::info!(
            action,
            path,
            ok,
            elevated = true,
            target_user,
            source = "manual",
            "file browser change with elevated rights"
        );
    }
}

pub(super) async fn lock_browser_sftp<'a>(
    session: &'a Session,
    channel: &BrowserChannel,
) -> BrowserSftpGuard<'a> {
    match &channel.kind {
        BrowserChannelKind::Normal => BrowserSftpGuard::Normal(session.lock_sftp().await),
        BrowserChannelKind::Elevated {
            expected_user,
            slot: Some(slot),
        } => BrowserSftpGuard::Elevated {
            slot: slot.clone(),
            expected_user: expected_user.clone(),
            revoked_mid_command: channel.revoked_mid_command.clone(),
        },
        BrowserChannelKind::Elevated { slot: None, .. } => BrowserSftpGuard::ElevatedInactive,
    }
}

/// Liefert die Session und stellt den gewählten Kanal bereit — gemeinsame
/// Vorbedingung aller `sftp_*`-Befehle unten. Normal: öffnet die
/// SFTP-Verbindung bei Bedarf (Spec 0020, Abschnitt 3). Erhöht: nur, wenn
/// der erhöhte Kanal bereits aktiv ist; er wird hier nie implizit geöffnet.
async fn browser_session(
    state: &AppState,
    session_id: SessionId,
    channel: &BrowserChannel,
) -> CommandResult<Arc<Session>> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    match &channel.kind {
        BrowserChannelKind::Normal => app_logic::orchestration::ensure_sftp_open(&session).await?,
        BrowserChannelKind::Elevated { slot, .. } => {
            if slot.is_none() {
                return Err(ELEVATED_CHANNEL_INACTIVE.into());
            }
        }
    }
    Ok(session)
}

/// Spec 0067, A1–A3: schaltet den erhöhten Dateibrowser-Kanal ein (`sudo
/// -n <sftp-server>` über einen Exec-Kanal). Nur passwortloses sudo; ein
/// Fehlschlag kommt als strukturierte `failure` zurück (inkl. sudoers-Zeile),
/// nicht als `Err`. Nie persistiert — lebt nur in der Session.
#[tauri::command]
pub async fn sftp_elevation_enable(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    target_user: Option<String>,
) -> CommandResult<app_logic::dto::ElevationResultDto> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    // Transport-Grenze statt Sonderfall in der Logik: der lokale
    // Pseudo-Server hat kein sudo/sftp-server (CLAUDE.md, "No
    // special-casing ... in the core loop").
    if app_logic::dto::is_local(session.server_id) {
        return Ok(app_logic::dto::ElevationResultDto {
            active: false,
            target_user: target_user.unwrap_or_else(|| {
                ssh_manager_core::ssh::elevated::DEFAULT_ELEVATION_USER.to_string()
            }),
            sftp_server_path: None,
            failure: Some(app_logic::dto::ElevationFailureDto {
                kind: app_logic::dto::ElevationFailureKind::Unsupported,
                sudoers_line: None,
                detail: None,
            }),
        });
    }
    let server = state.profile_store.get_server(&session.server_id).await?;
    crate::elevated_sftp::enable(
        &ElevationContext {
            sessions: &state.sessions,
            registry: elevated.inner(),
            session_id,
            session: &session,
        },
        &server.username,
        server.sftp_server_path.as_deref(),
        target_user.as_deref(),
        &BrowserAccess(()),
    )
    .await
}

/// Spec 0067, A5: schaltet den erhöhten Kanal aus. Das Frontend ruft das
/// auch beim Öffnen des Browsers auf, damit ein evtl. noch offener Kanal nie
/// unbemerkt aktiv bleibt.
///
/// Spec 0084, §9 (Klarstellung 2026-09-28): Der Kanal wird nicht nur
/// ausgehängt, sondern widerrufen — ein Befehl, der ihn gerade festhält,
/// kann ihn danach nicht mehr benutzen; der Widerruf wartet auf einen
/// laufenden Vorgang. Eine unbekannte Sitzungskennung liefert wie bisher
/// `Err("Session nicht gefunden")` (T9).
#[tauri::command]
pub async fn sftp_elevation_disable(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
) -> CommandResult<()> {
    crate::elevated_sftp::disable(
        &state.sessions,
        elevated.inner(),
        session_id,
        &BrowserAccess(()),
    )
    .await
}

/// Spec 0067, A5: Ziel-Nutzer des aktiven erhöhten Kanals, `None` = aus.
#[tauri::command]
pub async fn sftp_elevation_status(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
) -> CommandResult<Option<String>> {
    state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    let Some(slot) = elevated.slot(session_id, &BrowserAccess(())) else {
        return Ok(None);
    };
    let guard = slot.lock_owned(&BrowserAccess(())).await;
    Ok(guard.as_ref().map(|elevated| elevated.target_user.clone()))
}

pub(super) fn file_name_of(path: &str) -> String {
    // Führender Trim gegen einen abschließenden `/` (z. B. `/srv/data/`) —
    // ohne ihn liefert `rsplit('/').next()` einen leeren String, der
    // Filter greift, und der `unwrap_or(path)`-Fallback gibt versehentlich
    // den GESAMTEN Pfad statt nur seines letzten Segments zurück (Spec-
    // Reviewer-Fund, Spec 0054, Review des Gesamtpakets).
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets, ERHÖHTE
/// Priorität): jeder Punkt, an dem ein vom SFTP-SERVER gelieferter Name
/// (`RemoteEntry::name`, oder ein daraus über `file_name_of` abgeleiteter
/// Name) als LOKALES Pfadsegment verwendet wird (rekursiver Ordner-
/// Download, "Lokal öffnen"), ist ein klassisches Zip-Slip-Risiko: ein
/// (kompromittierter oder fehlerhaft implementierter) Server könnte statt
/// eines normalen Dateinamens `"../../.zshrc"` oder einen absoluten Pfad
/// wie `"/Users/u/.ssh/authorized_keys"` liefern — `PathBuf::join(..)`
/// verlässt bei `..`-Segmenten das Zielverzeichnis, und bei einem
/// absoluten Pfad ERSETZT `join()` den kompletten bisherigen Präfix statt
/// ihn anzuhängen. Lehnt jeden Namen ab, der nicht GENAU EIN normales
/// Pfadsegment ist (kein `.`/`..`, kein eingebetteter Separator, keine
/// führende Root/Präfix-Komponente) — funktioniert plattformunabhängig,
/// da `Path::components()` die jeweils betriebssystemeigene
/// Separator-/Präfix-Erkennung übernimmt.
pub(super) fn safe_local_segment(name: &str) -> CommandResult<()> {
    use std::path::{Component, Path};
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(CommandError::from(format!(
            "Unsicherer Dateiname vom Server abgelehnt: '{name}'"
        ))),
    }
}

#[cfg(test)]
mod safe_local_segment_tests {
    //! Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets, ERHÖHTE
    //! Priorität): Zip-Slip über servergelieferte Dateinamen beim
    //! rekursiven Ordner-Download/"Lokal öffnen". `download_recursive`/
    //! `download_entry_to`/`sftp_open_for_editing` rufen `safe_local_segment`
    //! jetzt vor jedem `PathBuf::join(entry.name)` auf — diese Tests prüfen
    //! nur die reine Funktion (`std::path::Path::join`s Verhalten bei
    //! `..`-Segmenten/absoluten Pfaden ist dokumentiertes, stabiles
    //! Standardbibliotheks-Verhalten, kein separat zu beweisender Teil).
    use super::*;

    #[test]
    fn test_accepts_a_plain_file_name() {
        assert!(safe_local_segment("readme.md").is_ok());
        assert!(safe_local_segment("nginx.conf.smartssh-backup-123").is_ok());
    }

    #[test]
    fn test_rejects_parent_directory_traversal() {
        assert!(safe_local_segment("..").is_err());
        assert!(safe_local_segment("../etc/passwd").is_err());
        assert!(safe_local_segment("../../.zshrc").is_err());
    }

    #[test]
    fn test_rejects_current_directory_segment() {
        assert!(safe_local_segment(".").is_err());
    }

    #[test]
    fn test_rejects_an_embedded_separator() {
        assert!(safe_local_segment("a/b").is_err());
    }

    #[test]
    fn test_rejects_an_absolute_path() {
        assert!(safe_local_segment("/etc/passwd").is_err());
        assert!(safe_local_segment("/Users/u/.ssh/authorized_keys").is_err());
    }
}

#[cfg(test)]
mod browser_channel_tests {
    use ssh_manager_core::ssh::{CommandOutput, PtySize, SshError, SshTransport};

    use super::*;

    struct NoTransport;
    #[async_trait::async_trait]
    impl SshTransport for NoTransport {
        async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
            unreachable!()
        }
        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn ssh_manager_core::ssh::InteractiveShell>, SshError> {
            unreachable!()
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            Ok(())
        }
    }

    /// Spec 0084, A1: der erhöhte Kanal liegt in der Zuordnung, nicht an der
    /// `Session` — Tests bauen ihre Ausgangslage deshalb hier auf.
    fn registry_with_channel(
        session_id: SessionId,
        target_user: &str,
        sftp: ssh_manager_core::ssh::mock::MockSftpSession,
    ) -> ElevatedSftpRegistry {
        let registry = ElevatedSftpRegistry::default();
        registry.insert_for_tests(
            session_id,
            crate::elevated_sftp::ElevatedSftp {
                target_user: target_user.to_string(),
                sftp: Box::new(sftp),
            },
        );
        registry
    }

    #[tokio::test]
    async fn test_elevated_request_without_active_channel_fails_instead_of_falling_back() {
        let session = app_logic::test_support::session_with_transport(Box::new(NoTransport));
        session
            .set_sftp_for_tests(Box::new(ssh_manager_core::ssh::mock::MockSftpSession::new()))
            .await;
        let registry = ElevatedSftpRegistry::default();
        let session_id = SessionId::new_v4();

        let channel = BrowserChannel::from_request(&registry, session_id, Some("root".to_string()));
        let mut guard = lock_browser_sftp(&session, &channel).await;
        let err = guard
            .sftp()
            .await
            .err()
            .expect("ohne aktiven erhöhten Kanal muss es scheitern");
        assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
    }

    /// spec-reviewer-Fund (Spec 0067): läuft der erhöhte Kanal inzwischen
    /// als ein anderer Nutzer als erwartet (z. B. Edit-Flow als www-data
    /// geöffnet, danach als root neu eingeschaltet), scheitert die Aktion.
    #[tokio::test]
    async fn test_elevated_request_for_another_user_than_active_fails() {
        let session = app_logic::test_support::session_with_transport(Box::new(NoTransport));
        let session_id = SessionId::new_v4();
        let registry = registry_with_channel(
            session_id,
            "root",
            ssh_manager_core::ssh::mock::MockSftpSession::new(),
        );

        let channel =
            BrowserChannel::from_request(&registry, session_id, Some("www-data".to_string()));
        let mut guard = lock_browser_sftp(&session, &channel).await;
        let err = guard
            .sftp()
            .await
            .err()
            .expect("anderer Nutzer als erwartet muss scheitern");
        assert!(err.message.contains("www-data"), "{}", err.message);
    }

    #[tokio::test]
    async fn test_elevated_request_uses_the_elevated_channel_not_the_normal_one() {
        let session = app_logic::test_support::session_with_transport(Box::new(NoTransport));
        let normal = ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "USER");
        let elevated = ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT");
        session.set_sftp_for_tests(Box::new(normal)).await;
        let session_id = SessionId::new_v4();
        let registry = registry_with_channel(session_id, "root", elevated);

        let channel = BrowserChannel::from_request(&registry, session_id, Some("root".to_string()));
        let mut guard = lock_browser_sftp(&session, &channel).await;
        assert_eq!(
            guard.sftp().await.unwrap().read_file("/x").await.unwrap(),
            b"ROOT"
        );
        drop(guard);
        let channel = BrowserChannel::from_request(&registry, session_id, None);
        let mut guard = lock_browser_sftp(&session, &channel).await;
        assert_eq!(
            guard.sftp().await.unwrap().read_file("/x").await.unwrap(),
            b"USER"
        );
    }

    /// Spec 0084, T7: Zwei gleichzeitige Sitzungen zum **selben** Server,
    /// der erhöhte Kanal ist nur in A aktiv. B muss über den normalen Kanal
    /// laufen, und Ausschalten in B darf A nicht berühren. Scheitert, wenn
    /// die Zuordnung nicht an der Sitzung hängt (z. B. am Server).
    #[tokio::test]
    async fn test_t7_an_elevated_channel_belongs_to_its_session_not_to_the_server() {
        let server_id = ssh_manager_core::shared::ServerId::new();
        let mut session_a = app_logic::test_support::session_with_transport(Box::new(NoTransport));
        let mut session_b = app_logic::test_support::session_with_transport(Box::new(NoTransport));
        session_a.server_id = server_id;
        session_b.server_id = server_id;
        session_b
            .set_sftp_for_tests(Box::new(
                ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "USER-B"),
            ))
            .await;

        let id_a = SessionId::new_v4();
        let id_b = SessionId::new_v4();
        let sessions = app_logic::session::SessionManager::new();
        let session_a = std::sync::Arc::new(session_a);
        let session_b = std::sync::Arc::new(session_b);
        sessions.insert(id_a, session_a.clone());
        sessions.insert(id_b, session_b.clone());
        let registry = registry_with_channel(
            id_a,
            "root",
            ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT-A"),
        );

        // B fragt den erhöhten Kanal an: es gibt keinen für B.
        let channel_b = BrowserChannel::from_request(&registry, id_b, Some("root".to_string()));
        assert!(
            channel_b.is_elevated() && !channel_b.has_elevated_slot(),
            "B darf den erhöhten Kanal von A nicht sehen"
        );
        let mut guard = lock_browser_sftp(&session_b, &channel_b).await;
        assert!(
            guard.sftp().await.is_err(),
            "eine erhöhte Aktion in B muss scheitern statt A's Kanal zu benutzen"
        );
        drop(guard);

        // B über den normalen Kanal: liest B's eigene Daten.
        let channel_b = BrowserChannel::from_request(&registry, id_b, None);
        let mut guard = lock_browser_sftp(&session_b, &channel_b).await;
        assert_eq!(
            guard.sftp().await.unwrap().read_file("/x").await.unwrap(),
            b"USER-B"
        );
        drop(guard);

        // Ausschalten in B lässt A unberührt.
        crate::elevated_sftp::disable(&sessions, &registry, id_b, &BrowserAccess(()))
            .await
            .expect("B ist bekannt, hat aber keinen Kanal — kein Fehler");
        let channel_a = BrowserChannel::from_request(&registry, id_a, Some("root".to_string()));
        let mut guard = lock_browser_sftp(&session_a, &channel_a).await;
        assert_eq!(
            guard.sftp().await.unwrap().read_file("/x").await.unwrap(),
            b"ROOT-A",
            "Ausschalten in B darf den Kanal von A nicht entfernen"
        );
    }

    // --- Spec 0084, §9 (Klarstellung 2026-09-28): Widerruf wirkt beim
    // Zugriff, nicht beim Befehlsbeginn ----------------------------------
    //
    // Angriffsbild aus dem Review: `sftp_download` schlägt den erhöhten
    // Kanal zu Befehlsbeginn nach und öffnet danach den Speichern-Dialog.
    // Schaltet der Nutzer währenddessen die erhöhten Rechte aus (oder auf
    // einen anderen Nutzer um, oder trennt die Sitzung), darf der bereits
    // festgehaltene Kanal danach nicht mehr benutzt werden.
    //
    // Alle drei Tests scheitern gegen die Variante „Kanal bei Befehlsbeginn
    // festhalten“, also gegen ein Widerrufen, das den Kanal weder
    // herausnimmt noch als widerrufen markiert.
    //
    // T10 und T10b laufen über `disable`/`enable`, die den Kanalwert
    // zusätzlich herausnehmen; T10c läuft über `remove_session`, das nur
    // markiert (damit das Trennen auf keinen Transfer wartet) — dort kann
    // der Fehler deshalb ausschließlich aus der Widerrufs-Prüfung in
    // `sftp()` stammen.

    /// Fängt genau den Ablauf ein, den ein Befehl durchläuft: Kanal zu
    /// Beginn nachschlagen, erste Nutzung, Wartezeit, zweite Nutzung.
    async fn held_channel(
        session_id: SessionId,
        registry: &ElevatedSftpRegistry,
        session: &Session,
    ) -> BrowserChannel {
        let channel = BrowserChannel::from_request(registry, session_id, Some("root".to_string()));
        let mut guard = lock_browser_sftp(session, &channel).await;
        assert_eq!(
            guard
                .sftp()
                .await
                .expect("Vorbedingung: die erste Nutzung gelingt")
                .read_file("/x")
                .await
                .unwrap(),
            b"ROOT"
        );
        channel
    }

    /// Spec 0084, T10: Kanal von einem Befehl festgehalten, Nutzer schaltet
    /// die erhöhten Rechte aus — die nächste Nutzung scheitert.
    #[tokio::test]
    async fn test_t10_a_held_channel_cannot_be_used_after_disabling() {
        let session = std::sync::Arc::new(app_logic::test_support::session_with_transport(
            Box::new(NoTransport),
        ));
        let session_id = SessionId::new_v4();
        let sessions = app_logic::session::SessionManager::new();
        sessions.insert(session_id, session.clone());
        let registry = registry_with_channel(
            session_id,
            "root",
            ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT"),
        );

        let channel = held_channel(session_id, &registry, &session).await;

        crate::elevated_sftp::disable(&sessions, &registry, session_id, &BrowserAccess(()))
            .await
            .expect("Ausschalten gelingt");

        let mut guard = lock_browser_sftp(&session, &channel).await;
        let err = guard
            .sftp()
            .await
            .err()
            .expect("nach dem Ausschalten darf der festgehaltene Kanal nicht mehr tragen");
        assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
    }

    /// Spec 0084, T10b: dasselbe, aber der Kanal wird zwischendurch für
    /// einen **anderen** Nutzer neu aktiviert. Der festgehaltene root-Kanal
    /// darf danach nicht weiterlaufen.
    #[tokio::test]
    async fn test_t10b_a_held_channel_cannot_be_used_after_reactivating_for_another_user() {
        let f = crate::test_support::elevation::fixture(Box::new(
            crate::test_support::elevation::working_transport(),
        ));
        // Ausgangslage über den echten Weg: erhöhte Rechte als root aktiv.
        crate::elevated_sftp::enable(&f.ctx(), "deploy", None, None, &BrowserAccess(()))
            .await
            .expect("Vorbedingung: Aktivieren als root gelingt");
        let channel =
            BrowserChannel::from_request(&f.registry, f.session_id, Some("root".to_string()));
        let mut guard = lock_browser_sftp(&f.session, &channel).await;
        assert!(
            guard.sftp().await.is_ok(),
            "Vorbedingung: die erste Nutzung gelingt"
        );
        drop(guard);

        crate::elevated_sftp::enable(
            &f.ctx(),
            "deploy",
            None,
            Some("www-data"),
            &BrowserAccess(()),
        )
        .await
        .expect("Neu-Aktivieren als www-data gelingt");

        let mut guard = lock_browser_sftp(&f.session, &channel).await;
        let err = guard
            .sftp()
            .await
            .err()
            .expect("der festgehaltene root-Kanal darf nach dem Umschalten nicht mehr tragen");
        assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
    }

    /// Spec 0084, T10c: dasselbe, aber die Sitzung wird über A2.1 entfernt
    /// (Trennen). Auch dann darf der festgehaltene Kanal nicht mehr tragen.
    #[tokio::test]
    async fn test_t10c_a_held_channel_cannot_be_used_after_the_session_was_removed() {
        let session = std::sync::Arc::new(app_logic::test_support::session_with_transport(
            Box::new(NoTransport),
        ));
        let session_id = SessionId::new_v4();
        let sessions = app_logic::session::SessionManager::new();
        sessions.insert(session_id, session.clone());
        let registry = registry_with_channel(
            session_id,
            "root",
            ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT"),
        );

        let channel = held_channel(session_id, &registry, &session).await;

        registry
            .remove_session(&sessions, session_id)
            .expect("die Sitzung war eingetragen");

        let mut guard = lock_browser_sftp(&session, &channel).await;
        let err = guard
            .sftp()
            .await
            .err()
            .expect("nach dem Trennen darf der festgehaltene Kanal nicht mehr tragen");
        assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
    }

    /// spec-reviewer-Fund (Runde 2): Das Trennen darf nicht hinter einem
    /// laufenden erhöhten Transfer stehenbleiben. Der Widerruf beim
    /// Entfernen einer Sitzung ist deshalb der sofort wirksame Merker, kein
    /// Warten auf die Kanal-Sperre — sonst reagierte ein Klick auf
    /// „Trennen" während eines hängenden Uploads sichtbar gar nicht.
    ///
    /// Scheitert (mit Zeitüberschreitung) gegen die Variante, die beim
    /// Entfernen auf die Kanal-Sperre wartet.
    #[tokio::test]
    async fn test_removing_a_session_does_not_wait_for_a_running_elevated_transfer() {
        let session = std::sync::Arc::new(app_logic::test_support::session_with_transport(
            Box::new(NoTransport),
        ));
        let session_id = SessionId::new_v4();
        let sessions = std::sync::Arc::new(app_logic::session::SessionManager::new());
        sessions.insert(session_id, session.clone());
        let registry = std::sync::Arc::new(registry_with_channel(
            session_id,
            "root",
            ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT"),
        ));

        // Ein laufender Transfer hält die Kanal-Sperre über seine ganze
        // Dauer — genau das tun `sftp_upload`/`download_one_file`.
        let held = registry
            .slot(session_id, &BrowserAccess(()))
            .expect("Vorbedingung: Kanal aktiv");
        let transfer_guard = held.lock_owned(&BrowserAccess(())).await;

        // In einem eigenen Thread, damit eine Variante, die doch auf die
        // Kanal-Sperre wartet, als Zeitüberschreitung sichtbar wird, statt
        // den ganzen Testlauf hängen zu lassen.
        let registry_for_removal = registry.clone();
        let sessions_for_removal = sessions.clone();
        let removal = tokio::task::spawn_blocking(move || {
            registry_for_removal.remove_session(&sessions_for_removal, session_id)
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), removal)
            .await
            .expect("Trennen darf nicht auf den laufenden Transfer warten")
            .expect("der Entfernen-Task darf nicht panisch enden")
            .expect("die Sitzung war eingetragen");

        assert!(
            held.is_revoked(),
            "der Widerruf muss schon wirken, während der Transfer die Sperre noch hält"
        );

        // Und der Transfer selbst ist ab sofort nicht mehr erhöht nutzbar.
        drop(transfer_guard);
        let channel = BrowserChannel::from_request(&registry, session_id, Some("root".to_string()));
        let mut guard = lock_browser_sftp(&session, &channel).await;
        assert!(guard.sftp().await.is_err());
    }

    /// spec-reviewer-Fund (Runde 2): Ein Browser-Befehl, der beim
    /// Umschalten des Ziel-Nutzers **bereits auf der Kanal-Sperre wartet**,
    /// bekommt sie vor dem Widerruf (die Sperre ist fair) — er darf den
    /// alten Kanal danach trotzdem nicht mehr benutzen.
    ///
    /// Scheitert gegen die Variante, die den Widerruf erst am Ende der
    /// Warteschlange wirken lässt (Widerruf nur per `take()` unter der
    /// Sperre, ohne sofort wirksamen Merker).
    #[tokio::test]
    async fn test_a_command_already_waiting_on_the_channel_fails_after_the_user_switched() {
        let f = crate::test_support::elevation::fixture(Box::new(
            crate::test_support::elevation::working_transport(),
        ));
        crate::elevated_sftp::enable(&f.ctx(), "deploy", None, None, &BrowserAccess(()))
            .await
            .expect("Vorbedingung: Aktivieren als root gelingt");

        // Befehl B hat den Kanal schon nachgeschlagen und wartet auf die
        // Sperre, die Befehl A gerade hält.
        let channel_b =
            BrowserChannel::from_request(&f.registry, f.session_id, Some("root".to_string()));
        let held = f
            .registry
            .slot(f.session_id, &BrowserAccess(()))
            .expect("Vorbedingung: Kanal aktiv");
        let guard_a = held.lock_owned(&BrowserAccess(())).await;

        // Der Nutzer schaltet auf www-data um. Das Aktivieren wartet
        // seinerseits auf A, der Widerruf des alten Kanals muss aber schon
        // vorher greifen.
        let f_for_switch = std::sync::Arc::new(f);
        let f_for_task = f_for_switch.clone();
        let switch = tokio::spawn(async move {
            crate::elevated_sftp::enable(
                &f_for_task.ctx(),
                "deploy",
                None,
                Some("www-data"),
                &BrowserAccess(()),
            )
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !held.is_revoked() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("der Widerruf muss wirken, bevor die Sperre frei wird");

        // Jetzt gibt A die Sperre frei, B kommt dran — und scheitert.
        drop(guard_a);
        let mut guard = lock_browser_sftp(&f_for_switch.session, &channel_b).await;
        let err = guard
            .sftp()
            .await
            .err()
            .expect("B darf nach dem Nutzerwechsel nicht mehr als root arbeiten");
        assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
        drop(guard);

        tokio::time::timeout(std::time::Duration::from_secs(5), switch)
            .await
            .expect("das Umschalten muss enden")
            .expect("der Umschalt-Task darf nicht panisch enden")
            .expect("das Umschalten selbst gelingt");
    }

    // --- Spec 0084, T5/T5b -------------------------------------------------
    //
    // Reine `orchestration`-Tests, aber sie müssen den erhöhten Kanal
    // aufbauen können (`ElevatedSftpRegistry`, `BrowserAccess`) — beides
    // bleibt nach Spec 0084 A1 unerreichbar für `app-logic`. Deshalb leben
    // sie hier statt in `app_logic::orchestration::chat_turn` (Modulpfad
    // darf sich ändern, A6.3 — die Testnamen bleiben wörtlich erhalten).

    /// Lässt jedes Kommando zu — für Tests, die die Filter-Engine nicht
    /// prüfen wollen, nur die Kanalwahl.
    struct AllowEverythingPolicyStore;
    #[async_trait::async_trait]
    impl ssh_manager_core::filter::PolicyStore for AllowEverythingPolicyStore {
        async fn rules_for(
            &self,
            _scope: &ssh_manager_core::filter::EffectiveScope,
        ) -> Vec<ssh_manager_core::filter::Rule> {
            vec![ssh_manager_core::filter::Rule {
                id: ssh_manager_core::filter::RuleId("allow-all".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Allow,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }

    /// Spec 0067, A5 (Regressionstest): ist der erhöhte Dateibrowser-Kanal
    /// aktiv, lesen und schreiben KI- und MCP-Aktionen trotzdem über den
    /// NORMALEN Kanal — der erhöhte ist nur für Browser-Commands da.
    #[tokio::test]
    async fn test_ai_and_mcp_file_actions_never_use_the_elevated_channel() {
        use ssh_manager_core::ai::AiEvent;
        use ssh_manager_core::profiles::AiAction;
        use ssh_manager_core::ssh::mock::MockSftpSession;

        use app_logic::confirmation::ConfirmationRegistry;
        use app_logic::dto::ActionUserDecision;
        use app_logic::events::TestEmitter;
        use app_logic::orchestration::{handle_mcp_action_proposed, run_chat_turn};
        use app_logic::test_support::InMemoryProfileStore;

        for origin in ["ai", "mcp"] {
            let ai_events = if origin == "ai" {
                vec![
                    AiEvent::ActionProposed(AiAction::ReadRemoteFile {
                        path: "/etc/secret.conf".to_string(),
                    }),
                    AiEvent::Done,
                ]
            } else {
                vec![AiEvent::Done]
            };
            let mut session = app_logic::test_support::session_with_ai_and_transport(
                crate::test_support::MockAiProvider::new(ai_events),
                Box::new(NoTransport),
            );
            session.filter_engine = Box::new(ssh_manager_core::filter::FilterEngine::new(
                AllowEverythingPolicyStore,
            ));
            let normal =
                MockSftpSession::new().with_file("/etc/secret.conf", b"USER-VIEW".to_vec());
            let elevated =
                MockSftpSession::new().with_file("/etc/secret.conf", b"ROOT-VIEW".to_vec());
            session.set_sftp_for_tests(Box::new(normal.clone())).await;
            // Spec 0084, A1: Der erhöhte Kanal hängt nicht mehr an der
            // `Session`, sondern an der Zuordnung in `app-shell`. Er wird
            // hier trotzdem aufgebaut — genau darum geht es: er ist aktiv,
            // und KI und MCP kommen trotzdem nicht an ihn heran.
            let session_id = SessionId::new_v4();
            let elevated_registry = registry_with_channel(session_id, "root", elevated.clone());

            let emitter = TestEmitter::default();
            let profile_store = InMemoryProfileStore::default();
            let confirmations = ConfirmationRegistry::new();
            if origin == "ai" {
                run_chat_turn(
                    &session,
                    session_id,
                    &emitter,
                    &profile_store,
                    &confirmations,
                )
                .await;
            } else {
                // MCP verlangt immer eine Bestätigung — hier genehmigt,
                // damit die Aktion wirklich ausgeführt wird.
                let action = handle_mcp_action_proposed(
                    &session,
                    session_id,
                    AiAction::ReadRemoteFile {
                        path: "/etc/secret.conf".to_string(),
                    },
                    &emitter,
                    &profile_store,
                    &confirmations,
                    Some("test-client".to_string()),
                );
                let responder = async {
                    loop {
                        let pending = emitter.events.lock().unwrap().iter().find_map(|(n, p)| {
                            (n == "chat-action-proposed")
                                .then(|| p["actionId"].as_str().unwrap().to_string())
                        });
                        if let Some(id) = pending {
                            let _ = confirmations
                                .resolve(&id.parse().unwrap(), ActionUserDecision::Approve);
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                };
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    tokio::join!(action, responder)
                })
                .await
                .expect("MCP-Aktion muss nach der Bestätigung enden");
            }

            let events = emitter.events.lock().unwrap().clone();
            let result = events
                .iter()
                .find(|(name, _)| name == "chat-action-result")
                .unwrap_or_else(|| panic!("{origin}: kein Ergebnis-Event"));
            let content = result.1["result"]["content"].as_str().unwrap();
            assert!(content.contains("USER-VIEW"), "{origin}: {content}");
            assert!(!content.contains("ROOT-VIEW"), "{origin}: {content}");
            assert!(
                elevated.calls().is_empty(),
                "{origin}: der erhöhte Kanal darf nie berührt werden, war: {:?}",
                elevated.calls()
            );
            assert!(
                !normal.calls().is_empty(),
                "{origin}: normaler Kanal wurde benutzt"
            );
            assert!(
                elevated_registry
                    .slot(session_id, &BrowserAccess(()))
                    .is_some(),
                "{origin}: Vorbedingung — der erhöhte Kanal war die ganze Zeit aktiv"
            );
        }
    }

    /// Spec 0084, T5b (Gegenstück zu T5 für eine **Schreib**aktion): Auch
    /// eine MCP-Schreibaktion läuft bei aktivem erhöhtem Kanal über den
    /// NORMALEN Kanal — sie darf die Datei nie mit den Rechten des erhöhten
    /// Kanals schreiben.
    #[tokio::test]
    async fn test_mcp_write_actions_never_use_the_elevated_channel() {
        use ssh_manager_core::ai::AiEvent;
        use ssh_manager_core::profiles::AiAction;
        use ssh_manager_core::ssh::mock::MockSftpSession;

        use app_logic::confirmation::ConfirmationRegistry;
        use app_logic::dto::ActionUserDecision;
        use app_logic::events::TestEmitter;
        use app_logic::orchestration::handle_mcp_action_proposed;
        use app_logic::test_support::InMemoryProfileStore;

        let mut session = app_logic::test_support::session_with_ai_and_transport(
            crate::test_support::MockAiProvider::new(vec![AiEvent::Done]),
            Box::new(NoTransport),
        );
        session.filter_engine = Box::new(ssh_manager_core::filter::FilterEngine::new(
            AllowEverythingPolicyStore,
        ));
        let normal = MockSftpSession::new().with_file("/etc/secret.conf", b"USER-VIEW".to_vec());
        let elevated = MockSftpSession::new().with_file("/etc/secret.conf", b"ROOT-VIEW".to_vec());
        session.set_sftp_for_tests(Box::new(normal.clone())).await;

        let session_id = SessionId::new_v4();
        let elevated_registry = registry_with_channel(session_id, "root", elevated.clone());

        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();
        let action = handle_mcp_action_proposed(
            &session,
            session_id,
            AiAction::WriteRemoteFile {
                path: "/etc/secret.conf".to_string(),
                content: "MCP-WRITE".to_string(),
            },
            &emitter,
            &profile_store,
            &confirmations,
            Some("test-client".to_string()),
        );
        let responder = async {
            loop {
                let pending = emitter.events.lock().unwrap().iter().find_map(|(n, p)| {
                    (n == "chat-action-proposed")
                        .then(|| p["actionId"].as_str().unwrap().to_string())
                });
                if let Some(id) = pending {
                    let _ =
                        confirmations.resolve(&id.parse().unwrap(), ActionUserDecision::Approve);
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(action, responder)
        })
        .await
        .expect("MCP-Schreibaktion muss nach der Bestätigung enden");

        assert!(
            elevated.calls().is_empty(),
            "der erhöhte Kanal darf beim Schreiben nie berührt werden, war: {:?}",
            elevated.calls()
        );
        assert_eq!(
            normal.file_content("/etc/secret.conf").as_deref(),
            Some(b"MCP-WRITE".as_slice()),
            "geschrieben wurde über den normalen Kanal"
        );
        assert_eq!(
            elevated.file_content("/etc/secret.conf").as_deref(),
            Some(b"ROOT-VIEW".as_slice()),
            "die Datei hinter dem erhöhten Kanal bleibt unverändert"
        );
        assert!(
            elevated_registry
                .slot(session_id, &BrowserAccess(()))
                .is_some(),
            "Vorbedingung — der erhöhte Kanal war die ganze Zeit aktiv"
        );
    }
}
