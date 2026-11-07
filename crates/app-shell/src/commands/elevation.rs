//! Spec 0067: erhöhter SFTP-Kanal für den Dateibrowser (Zugangsnachweis,
//! Kanalwahl, Ein-/Ausschalten) — Teil der Spec-0083-Aufteilung von
//! `commands.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::State;

use ssh_manager_core::ssh::{SftpSession, SshError};

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
    /// Issue #622: Zuordnung der laufenden Datei-Operationen (Abbrechen,
    /// Übersicht für den Bestätigungsdialog).
    ops: Arc<crate::file_ops::FileOps>,
    session_id: SessionId,
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
            ops: registry.ops().clone(),
            session_id,
        }
    }

    /// Issue #622: trägt eine Operation dieses Befehls ein. Nur Operationen
    /// auf dem erhöhten Kanal erscheinen im Bestätigungsdialog.
    pub(super) fn register_op(
        &self,
        id: uuid::Uuid,
        kind: crate::file_ops::OpKind,
        path: &str,
    ) -> crate::file_ops::OpGuard {
        let (sid, el) = (self.session_id, self.is_elevated());
        let g = self.ops.register(sid, id, kind, path, el); // #1193: activity
        #[rustfmt::skip]
        let BrowserChannelKind::Elevated { slot: Some(s), .. } = &self.kind else { return g };
        g.with_activity(s.activity_handle())
    }

    /// Issue #622: Ein Transfer wurde gestoppt, weil der erhöhte Kanal
    /// widerrufen wurde (Trennen, Neu-Aktivieren): wie ein Widerruf an der
    /// nächsten Operation — Vermerk setzen, bekannte Meldung (Spec 0085,
    /// A1.2; Spec 0086, A2.1).
    pub(super) fn revoked_error(&self) -> CommandError {
        self.revoked_mid_command.store(true, Ordering::SeqCst);
        elevated_inactive()
    }

    /// Issue #622: Ist der erhöhte Kanal dieses Befehls inzwischen widerrufen
    /// (oder nie dagewesen)? Transfers prüfen das vor **jedem** Stück, damit
    /// nach dem Schließen kein weiteres Stück mehr verarbeitet wird.
    pub(super) fn is_revoked(&self) -> bool {
        match &self.kind {
            BrowserChannelKind::Normal => false,
            BrowserChannelKind::Elevated {
                slot: Some(slot), ..
            } => slot.is_revoked(),
            BrowserChannelKind::Elevated { slot: None, .. } => true,
        }
    }

    /// Läuft dieser Befehl über den erhöhten Kanal? (Nicht: ob dieser noch
    /// aktiv ist.) `sftp_download` leitet daraus ab, dass eine lokale Kopie
    /// nur für den eigenen Nutzer lesbar angelegt wird.
    pub(super) fn is_elevated(&self) -> bool {
        matches!(self.kind, BrowserChannelKind::Elevated { .. })
    }

    /// Spec 0086, A2.1: Der Text, den ein Transfer-Ereignis
    /// (`sftp-transfer-finished`) für ein **gescheitertes** Teilergebnis
    /// tragen muss.
    ///
    /// Ein Widerruf, der erst bei der einzelnen Operation greift, kommt im
    /// Rumpf als `SshError::ChannelError(…)` an, dessen `Display` das Präfix
    /// `Channel-Fehler: ` vorsetzt. Das Befehlsergebnis wird danach zentral
    /// in [`with_browser_channel`] auf `ELEVATED_CHANNEL_INACTIVE` gesetzt —
    /// das Ereignis war da aber längst gesendet. Die Transferliste zeigte
    /// deshalb einen anderen Text als der Befehl.
    ///
    /// Bewusst **hier** und nicht im jeweiligen Transfer-Kern: dieselbe
    /// Übersetzung, aus demselben Widerrufs-Vermerk, den
    /// [`with_browser_channel`] liest — keine zweite, eigene Regel je Befehl
    /// (ADR 0078 §4). Alle anderen Fehler bleiben, wie sie sind (A2.2).
    pub(super) fn transfer_error_message(&self, err: &CommandError) -> String {
        if self.revoked_mid_command.load(Ordering::SeqCst) {
            ELEVATED_CHANNEL_INACTIVE.to_string()
        } else {
            err.message.clone()
        }
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

const ELEVATED_CHANNEL_INACTIVE_CODE: &str = "ELEVATED_CHANNEL_INACTIVE";

fn elevated_inactive() -> CommandError {
    CommandError::with_code(ELEVATED_CHANNEL_INACTIVE, ELEVATED_CHANNEL_INACTIVE_CODE)
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
        return Err(elevated_inactive());
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

/// Stabiler Fehlercode für `UserChanged`; das Frontend übersetzt ihn
/// (`errors.ELEVATED_USER_CHANGED`) und bietet „Upload as <user>" an.
const ELEVATED_USER_CHANGED_CODE: &str = "ELEVATED_USER_CHANGED";

impl ElevatedAccessError {
    fn into_command_error(self) -> CommandError {
        match self {
            Self::UserChanged { .. } => {
                CommandError::with_code(self.message(), ELEVATED_USER_CHANGED_CODE)
            }
            Self::Revoked => CommandError::with_code(self.message(), "ELEVATED_ACCESS_REVOKED"),
        }
    }

    fn message(&self) -> String {
        match self {
            Self::Revoked => ELEVATED_CHANNEL_INACTIVE.to_string(),
            Self::UserChanged { active, expected } => format!(
                "Elevated mode now runs as \"{active}\", not as \"{expected}\" — the action was \
                 not performed."
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
    slot.touch(); // Issue #1193: any elevated access is activity
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
            Self::ElevatedInactive => Err(elevated_inactive()),
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
                        Err(err.into_command_error())
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
    pub(super) fn elevated_user(&self) -> Option<ElevatedAudit> {
        match self {
            Self::Normal(_) | Self::ElevatedInactive => None,
            // Ein widerrufener Kanal meldet keinen Ziel-Nutzer mehr: über
            // ihn läuft ohnehin keine Aktion mehr (s. `sftp`), und die
            // Audit-Zeile soll keine Erhöhung behaupten, die es nicht gab.
            Self::Elevated { slot, .. } if slot.is_revoked() => None,
            // Der Ziel-Nutzer des Kanals, nicht bloß der angefragte: `sftp()`
            // lässt keine Operation zu, solange beide auseinanderliegen
            // (`UserChanged`), beide sind hier also gleich.
            Self::Elevated {
                slot,
                expected_user,
                ..
            } => Some(ElevatedAudit {
                user: expected_user.clone(),
                sudo_auth: slot.sudo_auth(),
                server_logging: slot.server_logging(),
            }),
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

mod audit;
mod sftp_impl;
pub(super) use audit::{audit_elevated_change, ElevatedAudit};

/// Spec 0067, A5 (baut auf der Audit-Erfassbarkeit aus 0054 auf): jede
/// server-verändernde Browser-Aktion im erhöhten Modus hinterlässt eine
/// strukturierte Log-Zeile mit Kennzeichnung "erhöht" + Ziel-Nutzer, Quelle
/// "manuell". Nur Aktion und Pfade, nie Dateiinhalte.
/// Auch bei einem Fehlschlag protokolliert (`ok = false`) — ein rekursives
/// Löschen/chmod kann mittendrin scheitern, nachdem schon Einträge geändert
/// wurden.
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
    let session = app_logic::session::require_session(&state.sessions, session_id)?;
    match &channel.kind {
        BrowserChannelKind::Normal => app_logic::orchestration::ensure_sftp_open(&session).await?,
        BrowserChannelKind::Elevated { slot, .. } => {
            if slot.is_none() {
                return Err(elevated_inactive());
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
    confirm_stop: Option<bool>,
) -> CommandResult<app_logic::dto::ElevationResultDto> {
    let session = app_logic::session::require_session(&state.sessions, session_id)?;
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
            warning: None,
            running_operations: Vec::new(),
            server_logging: false,
        });
    }
    let server = state.profile_store.get_server(&session.server_id).await?;
    crate::elevated_sftp::enable_for_server(
        &ElevationContext {
            sessions: &state.sessions,
            registry: elevated.inner(),
            session_id,
            session: &session,
        },
        &server,
        target_user.as_deref(),
        confirm_stop.unwrap_or(false),
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
/// `Err("session not found")` (T9).
#[tauri::command]
pub async fn sftp_elevation_disable(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    confirm_stop: Option<bool>,
) -> CommandResult<app_logic::dto::ElevationDisableDto> {
    crate::elevated_sftp::disable(
        &state.sessions,
        elevated.inner(),
        session_id,
        confirm_stop.unwrap_or(false),
        &BrowserAccess(()),
    )
    .await
}

/// Issue #1194, Spec 0067 A5: whether the server profile of this session
/// allows elevated rights (the file browser hides its controls otherwise).
/// An unreadable profile is an `Err`; the frontend then offers nothing.
#[tauri::command]
pub async fn sftp_elevation_allowed(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<bool> {
    let session = app_logic::session::require_session(&state.sessions, session_id)?;
    // The local pseudo-server has no profile row; it is handled at the
    // transport boundary (`sftp_elevation_enable` answers `Unsupported`).
    if app_logic::dto::is_local(session.server_id) {
        return Ok(true);
    }
    Ok(state
        .profile_store
        .get_server(&session.server_id)
        .await?
        .elevation_allowed)
}

/// Spec 0067, A5: Ziel-Nutzer des aktiven erhöhten Kanals, `None` = aus.
#[tauri::command]
pub async fn sftp_elevation_status(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
) -> CommandResult<Option<String>> {
    app_logic::session::require_session(&state.sessions, session_id)?;
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
        _ => Err(CommandError::with_code(
            format!("Unsicherer Dateiname vom Server abgelehnt: '{name}'"),
            "SFTP_UNSAFE_FILE_NAME",
        )),
    }
}

#[cfg(test)]
mod tests_safe_segment;

#[cfg(test)]
mod tests_browser_channel;
