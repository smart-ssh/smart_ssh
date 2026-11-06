//! Spec 0067, Teil A: der erhöhte SFTP-Kanal einer Session (`sudo -n
//! <sftp-server>` über einen Exec-Kanal). Getrennt vom normalen
//! `Session::sftp`, das auch KI-Aktionen (`read_remote_file`/
//! `write_remote_file`) nutzen.
//!
//! **Nur Browser-Commands kommen heran:** [`ElevatedSftpSlot::lock_owned`]
//! und jeder Zugriff auf die [`ElevatedSftpRegistry`] verlangen ein
//! [`crate::commands::BrowserAccess`], das nur im Modul `commands` erzeugt
//! werden kann. KI (`orchestration`) und MCP (`mcp_backend`) können den
//! Kanal damit nicht einmal ansprechen — ein Versuch scheitert schon beim
//! Kompilieren.
//!
//! **Spec 0084, A1:** Der Kanal hängt seit diesem Schritt nicht mehr am
//! `Session`-Wert, sondern an der [`ElevatedSftpRegistry`] — einem eigenen,
//! von Tauri verwalteten Zustand von `app-shell` (s. `crate::lib::run`).
//! Damit bleibt er in `app-shell`, wenn die übrige Anwendungslogik (und mit
//! ihr `Session`) in einen Tauri-freien Crate zieht.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use ssh_manager_core::{ssh::SftpSession, sync::lock_tolerating_poison};

use crate::commands::BrowserAccess;
use crate::file_ops::{FileOps, StopReason, STOP_WAIT};
use app_logic::session::{Session, SessionManager};
use app_logic::state::SessionId;

/// How sudo authenticated when the elevated channel was opened (audit only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SudoAuth {
    #[default]
    Nopasswd,
    Password,
}

#[cfg(test)]
impl ElevatedSftp {
    /// Channel opened with a passwordless sudo rule.
    pub fn new(target_user: String, sftp: Box<dyn SftpSession>) -> Self {
        Self {
            target_user,
            sudo_auth: SudoAuth::Nopasswd,
            server_logging: false,
            sftp,
        }
    }
}

impl SudoAuth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nopasswd => "nopasswd",
            Self::Password => "password",
        }
    }
}

pub struct ElevatedSftp {
    /// Ziel-Nutzer der Rechteerhöhung (Default `root`).
    pub target_user: String,
    /// How sudo authenticated for this channel (audit line of each change).
    pub sudo_auth: SudoAuth,
    /// Issue #1192: `sftp-server` was started with `-l INFO -f AUTHPRIV`.
    pub server_logging: bool,
    pub sftp: Box<dyn SftpSession>,
}

/// Der erhöhte Kanal **einer** Sitzung mit seiner eigenen Sperre.
///
/// Nie persistiert: lebt nur in der [`ElevatedSftpRegistry`] und verschwindet
/// mit dem Eintrag dort (Verbindungstrennung, App-Neustart).
///
/// Spec 0084, §4: Die Sperre der Zuordnung (`ElevatedSftpRegistry`) gilt nur
/// für Nachschlagen, Eintragen und Entfernen; der Kanal selbst behält diese
/// eigene Sperre je Sitzung, damit ein laufender Vorgang in einer Sitzung das
/// Trennen einer anderen nicht aufhält. Der `Arc` macht die Sperre
/// unabhängig von der Karte haltbar: ein Browser-Befehl, der den Kanal
/// gerade nutzt, hält seinen eigenen Zeiger, während die Zuordnung schon
/// wieder frei ist.
#[derive(Clone, Default)]
pub struct ElevatedSftpSlot {
    channel: Arc<AsyncMutex<Option<ElevatedSftp>>>,
    /// Copy of the channel's `sudo_auth`, readable without the channel lock.
    sudo_auth: SudoAuth,
    /// Copy of the channel's `server_logging` (Issue #1192).
    server_logging: bool,
    /// Spec 0084, §9 (Klarstellung 2026-09-28): der **sofort** wirksame
    /// Teil des Widerrufs.
    ///
    /// Er steht neben der Kanal-Sperre, nicht in ihr, und das ist der
    /// Punkt: Ein Widerruf, der nur den Kanal unter der Sperre herausnimmt,
    /// wirkt erst, wenn er in der Warteschlange dieser Sperre an der Reihe
    /// ist — `tokio::sync::Mutex` ist fair, ein bereits wartender
    /// Browser-Befehl käme also noch **vor** dem Widerruf dran und liefe
    /// mit den alten Rechten weiter (spec-reviewer, Runde 2). Der Merker
    /// wird gesetzt, bevor irgendjemand die Sperre bekommen kann, und jeder
    /// Zugriff prüft ihn — damit endet die Nutzbarkeit im selben Moment, in
    /// dem der Nutzer ausschaltet, umschaltet oder trennt.
    revoked: Arc<std::sync::atomic::AtomicBool>,
    /// Issue #1193: time of the last elevated activity (idle timer).
    activity: idle::Activity,
    /// Issue #1193: wakes the idle timer task when the channel is revoked.
    revoked_wake: Arc<tokio::sync::Notify>,
    /// Nur für Tests (Spec 0085, §6): ein Haltepunkt **hinter** dem
    /// Schnellabbruch einer Operation und **vor** dem Anfordern der
    /// Kanal-Sperre — s. `commands::elevation::elevated_operation`.
    ///
    /// Er gibt einem Test den einen Moment, in dem ein Widerruf am
    /// Schnellabbruch schon vorbei ist und deshalb von der maßgeblichen
    /// Prüfung unter der Sperre gefangen werden muss. Ohne ihn wäre das nur
    /// zufällig zu treffen: es bräuchte einen zweiten Browser-Befehl, der in
    /// der Warteschlange der Sperre hängt, während widerrufen wird. Zwei Tests
    /// hängen daran — T6c (`sftp_exists` hat nur **eine** Operation, es gibt
    /// also keinen zweiten Zugriff, an dem ein Widerruf von selbst sichtbar
    /// würde) und der Regressionstest zur maßgeblichen Prüfung
    /// (spec-reviewer, Runde 1/2).
    ///
    /// Vorbild und Begründung wie bei
    /// [`ElevatedSftpRegistry::interleave_hook`] (Spec 0084, T8b). Existiert
    /// in keinem Produktivbau.
    #[cfg(test)]
    before_operation_hook: Arc<StdMutex<Option<BeforeOperationHook>>>,
}

/// s. [`ElevatedSftpSlot::before_operation_hook`].
#[cfg(test)]
type BeforeOperationHook = Box<dyn Fn() + Send>;

impl ElevatedSftpSlot {
    fn with_channel(channel: ElevatedSftp) -> Self {
        Self {
            sudo_auth: channel.sudo_auth,
            server_logging: channel.server_logging,
            channel: Arc::new(AsyncMutex::new(Some(channel))),
            revoked: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            activity: idle::Activity::default(),
            revoked_wake: Arc::new(tokio::sync::Notify::new()),
            #[cfg(test)]
            before_operation_hook: Arc::new(StdMutex::new(None)),
        }
    }

    /// Nur für T6c — s. [`Self::before_operation_hook`]. Der Haltepunkt wird
    /// beim ersten Durchlauf verbraucht. Wirkt auf **alle** Klone dieses
    /// Slots (gemeinsamer `Arc`), also auch auf den, den ein laufender
    /// Befehl gerade festhält.
    #[cfg(test)]
    pub(crate) fn set_before_operation_hook(&self, hook: BeforeOperationHook) {
        *lock_tolerating_poison(&self.before_operation_hook) = Some(hook);
    }

    /// Ruft den Haltepunkt aus [`Self::before_operation_hook`], falls einer
    /// gesetzt ist. Im Produktivbau ein leerer Rumpf.
    pub(crate) fn run_before_operation_hook(&self) {
        #[cfg(test)]
        {
            let hook = lock_tolerating_poison(&self.before_operation_hook).take();
            if let Some(hook) = hook {
                hook();
            }
        }
    }

    pub(crate) async fn lock_owned(
        &self,
        _access: &BrowserAccess,
    ) -> OwnedMutexGuard<Option<ElevatedSftp>> {
        self.channel.clone().lock_owned().await
    }

    /// Ist dieser Kanal widerrufen? Jeder Zugriff prüft das (s.
    /// `commands::elevation::BrowserSftpGuard::sftp`) — auch dann, wenn der
    /// Kanalwert selbst noch dasteht, weil ein Halter die Sperre gerade
    /// belegt.
    pub(crate) fn sudo_auth(&self) -> SudoAuth {
        self.sudo_auth
    }

    pub(crate) fn server_logging(&self) -> bool {
        self.server_logging
    }

    pub(crate) fn is_revoked(&self) -> bool {
        self.revoked.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Spec 0084, §9 (Klarstellung 2026-09-28): **Widerruf wirkt beim
    /// Zugriff, nicht beim Befehlsbeginn.** Wirkt sofort und ohne auf
    /// irgendeine Sperre zu warten — wer den Slot noch aus einem laufenden
    /// Befehl hält, scheitert ab hier mit `ELEVATED_CHANNEL_INACTIVE`,
    /// statt nach dem Ausschalten, dem Umschalten auf einen anderen Nutzer
    /// oder dem Trennen der Sitzung weiter mit erhöhten Rechten zu
    /// arbeiten.
    ///
    /// Bewusst **ohne** `BrowserAccess`: Widerrufen ist kein Zugriff, es
    /// nimmt nur weg. Sonst könnte `remove_session` (A2.1) den Kanal einer
    /// getrennten Sitzung gar nicht widerrufen.
    fn mark_revoked(&self) {
        self.revoked
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.revoked_wake.notify_one();
    }

    /// Issue #1193: records elevated activity for the idle timer.
    pub(crate) fn touch(&self) {
        self.activity.touch();
    }

    pub(crate) fn activity_handle(&self) -> idle::Activity {
        self.activity.clone()
    }

    fn activity(&self) -> &idle::Activity {
        &self.activity
    }

    fn revoked_wake(&self) -> &tokio::sync::Notify {
        &self.revoked_wake
    }

    fn same_channel(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.channel, &other.channel)
    }

    /// Widerruft (sofort, s. [`Self::mark_revoked`]) und nimmt den Kanal
    /// danach unter seiner eigenen Sperre heraus, damit er auch wirklich
    /// fallengelassen wird statt bis zum Ende des Programms zu leben.
    ///
    /// Das Herausnehmen **wartet**, bis ein gerade laufender Vorgang auf
    /// dem Kanal fertig ist — §9: „Deaktivieren wartet wie bisher". Für die
    /// Sicherheit ist das nicht mehr nötig (der Merker wirkt schon), nur
    /// für das saubere Schließen. Deshalb ruft der Trennpfad (A2.1)
    /// bewusst nur [`Self::mark_revoked`] und wartet nicht: ein „Trennen"
    /// darf nicht hinter einem hängenden Transfer stehenbleiben
    /// (spec-reviewer, Runde 2).
    ///
    /// Darf nie unter der Zuordnungs-Sperre und nie unter der
    /// Transport-Sperre einer Sitzung aufgerufen werden — sonst entsteht
    /// mit `commands::disconnect` (Kanal → Transport) ein Sperrzyklus.
    async fn revoke(&self) -> bool {
        self.mark_revoked();
        self.channel.lock().await.take().is_some()
    }
}

/// Spec 0084, A1/A2: Zuordnung Sitzungskennung → erhöhter Kanal.
///
/// Eigener, von Tauri verwalteter Zustand von `app-shell` (kein Feld von
/// `AppState`), damit der erhöhte Kanal auch dann in `app-shell` bleibt,
/// wenn `AppState`/`Session` in den Tauri-freien Logik-Crate ziehen.
///
/// **Sperrdisziplin (A2).** Die `slots`-Sperre ist die eine Sperre, unter der
/// ein Eintrag entsteht ([`Self::insert_if_session_alive`]) und verschwindet
/// ([`Self::take`], [`Self::remove_session`]). Das Eintragen prüft unter
/// dieser Sperre, dass die Sitzung noch im [`SessionManager`] steht; das
/// Entfernen einer Sitzung nimmt sie **nach** `SessionManager::remove`. Aus
/// beidem folgt, dass kein Eintrag seine Sitzung überleben kann:
/// * Eintrag vor `SessionManager::remove` → [`Self::remove_session`] löscht ihn.
/// * Eintrag nach `SessionManager::remove` → die Sitzung ist weg, die Prüfung
///   schlägt fehl, der Kanal wird verworfen.
///
/// Die umgekehrte Reihenfolge in [`Self::remove_session`] (erst Eintrag, dann
/// Sitzung) wäre genau die Lücke, in der ein gleichzeitiges Aktivieren einen
/// Eintrag für eine gleich darauf entfernte Sitzung anlegt — Regressionstest
/// `test_t8b_activation_between_the_two_removal_steps_leaves_no_entry`.
#[derive(Default)]
pub struct ElevatedSftpRegistry {
    slots: Arc<StdMutex<HashMap<SessionId, ElevatedSftpSlot>>>,
    /// Issue #1193: idle timeout setting and switch-off notification.
    idle: Arc<idle::IdleShared>,
    /// Issue #622: laufende Datei-Operationen aller Sitzungen. Jede Stelle
    /// unten, die einen Kanal widerruft, stoppt auch dessen Operationen.
    ops: Arc<FileOps>,
    /// Nur für den Regressionstest T8b (Spec 0084, §6): ein Haltepunkt
    /// **zwischen** den beiden Schritten von [`Self::remove_session`]. Der
    /// Test lässt dort ein gleichzeitiges Aktivieren vollständig
    /// durchlaufen; ohne diesen Haltepunkt wäre die Verschränkung nur
    /// zufällig zu treffen. Existiert in keinem Produktivbau.
    #[cfg(test)]
    interleave_hook: StdMutex<Option<InterleaveHook>>,
}

/// s. [`ElevatedSftpRegistry::interleave_hook`].
#[cfg(test)]
type InterleaveHook = Box<dyn Fn() + Send>;

impl ElevatedSftpRegistry {
    pub(crate) fn ops(&self) -> &Arc<FileOps> {
        &self.ops
    }

    pub(crate) fn idle(&self) -> &Arc<idle::IdleShared> {
        &self.idle
    }

    /// Issue #1193: one timer task per channel; it ends when the channel is
    /// revoked (any path) or replaced.
    fn start_idle_timer(&self, session_id: SessionId, slot: ElevatedSftpSlot) {
        let timer = idle::IdleTimer {
            slots: self.slots.clone(),
            ops: self.ops.clone(),
            shared: self.idle.clone(),
            session_id,
            slot,
        };
        match self.idle.spawner() {
            Some(spawn) => spawn(session_id, Box::pin(timer.run())),
            None => tracing::error!("no task scope for the elevated idle timer"),
        }
    }

    /// Only for tests of the idle timer: starts the timer of an inserted channel.
    #[cfg(test)]
    pub(crate) fn start_idle_timer_for_tests(&self, session_id: SessionId) {
        let slot = lock_tolerating_poison(&self.slots)
            .get(&session_id)
            .cloned();
        if let Some(slot) = slot {
            self.start_idle_timer(session_id, slot);
        }
    }

    /// Der erhöhte Kanal dieser Sitzung, sofern aktiv.
    pub(crate) fn slot(
        &self,
        session_id: SessionId,
        _access: &BrowserAccess,
    ) -> Option<ElevatedSftpSlot> {
        lock_tolerating_poison(&self.slots)
            .get(&session_id)
            .cloned()
    }

    /// Spec 0084, A2.2: trägt `channel` **nur** ein, wenn `session_id` zu
    /// diesem Zeitpunkt noch im `SessionManager` steht. Prüfung und Eintrag
    /// geschehen unter der `slots`-Sperre — derselben, unter der
    /// [`Self::remove_session`] den Eintrag löscht.
    ///
    /// `Err(())` heißt: nichts eingetragen, `channel` wird hier verworfen
    /// (sein `Drop` schließt den Exec-Kanal). `Ok(Some(slot))` liefert den
    /// **bisherigen** Eintrag zurück — der Aufrufer muss ihn widerrufen
    /// ([`ElevatedSftpSlot::revoke`]), sonst arbeitete ein Befehl, der ihn
    /// noch festhält, über das Neu-Aktivieren hinaus mit dem alten Kanal
    /// weiter (Spec 0084 §9, Klarstellung 2026-09-28).
    fn insert_if_session_alive(
        &self,
        sessions: &SessionManager,
        session_id: SessionId,
        channel: ElevatedSftp,
        _access: &BrowserAccess,
    ) -> Result<Option<ElevatedSftpSlot>, ()> {
        let mut slots = lock_tolerating_poison(&self.slots);
        if sessions.get(session_id).is_none() {
            return Err(());
        }
        let new_slot = ElevatedSftpSlot::with_channel(channel);
        let previous = slots.insert(session_id, new_slot.clone());
        self.start_idle_timer(session_id, new_slot);
        // Sofort, noch unter der Zuordnungs-Sperre: ab hier kann kein
        // Befehl den alten Kanal mehr benutzen — auch keiner, der schon auf
        // dessen Sperre wartet.
        if let Some(previous) = &previous {
            previous.mark_revoked();
            self.ops.stop_elevated(session_id, StopReason::Revoked);
        }
        Ok(previous)
    }

    /// Nimmt den Eintrag einer Sitzung aus der Zuordnung heraus und
    /// widerruft ihn sofort. Das Herausnehmen des Kanalwerts geschieht
    /// **danach** über [`ElevatedSftpSlot::revoke`] — es wartet auf einen
    /// laufenden Vorgang und darf deshalb nicht unter der
    /// Zuordnungs-Sperre laufen.
    fn take(&self, session_id: SessionId, _access: &BrowserAccess) -> Option<ElevatedSftpSlot> {
        let slot = lock_tolerating_poison(&self.slots).remove(&session_id);
        if let Some(slot) = &slot {
            slot.mark_revoked();
            self.ops.stop_elevated(session_id, StopReason::Revoked);
        }
        slot
    }

    /// Spec 0084, A2.1: **die eine** Funktion, die eine Sitzung samt ihrem
    /// erhöhten Kanal entfernt. Außer ihr und Tests ruft nichts
    /// `SessionManager::remove` auf.
    ///
    /// Reihenfolge ist Teil der Zusage: `SessionManager::remove` läuft
    /// **vor** dem Löschen des Zuordnungs-Eintrags (s. Sperrdisziplin am
    /// Typ). Der Transport der Sitzung wird hier nicht gesperrt — das
    /// Trennen bleibt danach in `commands::disconnect`.
    ///
    /// Spec 0084, §9 (Klarstellung 2026-09-28): Der Kanal wird zum Schluss
    /// auch **widerrufen**, nicht nur ausgehängt — ein Befehl, der ihn beim
    /// Trennen noch festhält, darf ihn danach nicht weiter benutzen.
    ///
    /// Der Widerruf ist hier bewusst nur der sofort wirksame Merker, ohne
    /// auf die Kanal-Sperre zu warten: Diese Funktion steht im
    /// `disconnect`-Pfad, und „Trennen" darf nicht hinter einem hängenden
    /// Transfer stehenbleiben (spec-reviewer, Runde 2 — sonst schließt sich
    /// der Tab nach einem Klick auf „Trennen" sichtbar nicht mehr). Der
    /// Kanalwert selbst verschwindet, sobald der letzte Halter ihn
    /// fallenlässt; nutzbar ist er ab sofort nicht mehr.
    pub(crate) fn remove_session(
        &self,
        sessions: &SessionManager,
        session_id: SessionId,
    ) -> Option<Arc<Session>> {
        let session = sessions.remove(session_id);
        #[cfg(test)]
        {
            let hook = lock_tolerating_poison(&self.interleave_hook).take();
            if let Some(hook) = hook {
                hook();
            }
        }
        let slot = lock_tolerating_poison(&self.slots).remove(&session_id);
        if let Some(slot) = slot {
            slot.mark_revoked();
            self.ops.stop_elevated(session_id, StopReason::Revoked);
        }
        session
    }

    /// Issue #355: a reconnect keeps the session but never its elevated
    /// channel (it belonged to the dead connection). The channel is revoked
    /// like on disconnect; the user elevates again.
    pub(crate) fn revoke_session(&self, session_id: SessionId) {
        let slot = lock_tolerating_poison(&self.slots).remove(&session_id);
        if let Some(slot) = slot {
            slot.mark_revoked();
            self.ops.stop_elevated(session_id, StopReason::Revoked);
        }
    }

    /// Nur für Tests, die einen aktiven Kanal als Ausgangslage brauchen,
    /// ohne den vollen Aktivierungs-Ablauf (Proben, sudo-Prüfung) zu fahren
    /// — die Prüfung aus A2.2 gehört zu genau diesem Ablauf und wird
    /// separat geprüft (T8/T8b).
    #[cfg(test)]
    pub(crate) fn insert_for_tests(&self, session_id: SessionId, channel: ElevatedSftp) {
        lock_tolerating_poison(&self.slots)
            .insert(session_id, ElevatedSftpSlot::with_channel(channel));
    }

    /// Nur für T8b — s. [`Self::interleave_hook`]. Der Haltepunkt wird beim
    /// ersten Durchlauf verbraucht.
    #[cfg(test)]
    fn set_interleave_hook(&self, hook: InterleaveHook) {
        *lock_tolerating_poison(&self.interleave_hook) = Some(hook);
    }
}

pub(crate) mod idle;
pub(crate) use idle::install as install_idle_timer;
pub(crate) use idle::Activity as IdleActivity;
mod probe;
pub(crate) use probe::run_probe;
use ssh_manager_core::ssh::elevated::{
    classify_sudo_check, elevated_sftp_command, elevated_sftp_password_command,
    is_plausible_sftp_server_path, is_valid_target_user, new_sudo_prompt_marker,
    parse_sftp_server_probe, parse_sftp_server_safety_probe, sftp_server_probe_command,
    sftp_server_safety_probe_command, sudo_check_command, sudoers_line, SftpServerSafety,
    SudoCheck, DEFAULT_ELEVATION_USER,
};

use app_logic::dto::{
    ElevationDisableDto, ElevationFailureDto, ElevationFailureKind, ElevationResultDto,
    ElevationWarningDto, ElevationWarningKind,
};
use app_logic::error::CommandResult;

/// Spec 0084, A1/A2: alles, was das Einschalten des erhöhten Kanals über die
/// Sitzungsgrenze hinweg braucht — gebündelt, weil `enable` sonst über die
/// Argumentgrenze liefe.
pub(crate) struct ElevationContext<'a> {
    pub sessions: &'a SessionManager,
    pub registry: &'a ElevatedSftpRegistry,
    pub session_id: SessionId,
    pub session: &'a Session,
}

/// Liefert bewusst `Ok`: ein gescheiterter Aktivierungsversuch ist ein
/// strukturiertes Ergebnis für die Oberfläche (Spec 0067, A1–A3), kein
/// Befehlsfehler. `Err` bleibt dem einen Fall aus Spec 0084, A2.2 vorbehalten
/// (Sitzung während des Aktivierens verschwunden).
fn failure(
    target_user: &str,
    sftp_server_path: Option<String>,
    kind: ElevationFailureKind,
    sudoers: Option<String>,
    detail: Option<String>,
) -> CommandResult<ElevationResultDto> {
    Ok(ElevationResultDto {
        active: false,
        target_user: target_user.to_string(),
        sftp_server_path,
        failure: Some(ElevationFailureDto {
            kind,
            sudoers_line: sudoers,
            detail,
        }),
        warning: None,
        running_operations: Vec::new(),
        server_logging: false,
    })
}

/// Spec 0067, A1–A3: schaltet den erhöhten Kanal für `ctx.session` ein.
/// `sudo -n`, oder (Issue #1191) mit dem gespeicherten sudo-Passwort; nie hängen (Timeouts), bei jedem Fehler eine
/// strukturierte Meldung statt eines kryptischen Fehlers. Die Probe-Ausgaben
/// (inkl. sudo-stderr) gehen nur in diese Meldung, nie in Chat/KI-Kontext.
///
/// Spec 0084, A2.2: Der geöffnete Kanal wird nur eingetragen, solange die
/// Sitzung noch steht — sonst `Err("session not found")` und der Kanal
/// wird verworfen.
///
/// **Sperr-Reihenfolge** (docs/architecture.md, "Lock hierarchy of a
/// session"): Der Widerruf des verdrängten Vorgängers steht
/// bewusst außerhalb des Blocks, der `session.transport` sperrt. Läge er
/// darin, entstünde mit `commands::disconnect` (das die Kanal-Sperre
/// freigibt und dann den Transport nimmt) ein Sperrzyklus
/// (spec-reviewer, Runde 2).
pub(crate) async fn enable(
    ctx: &ElevationContext<'_>,
    login: &str,
    override_path: Option<&str>,
    requested_user: Option<&str>,
    confirm_stop: bool,
    access: &BrowserAccess,
) -> CommandResult<ElevationResultDto> {
    let target_user = requested_user
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .unwrap_or(DEFAULT_ELEVATION_USER)
        .to_string();
    // Issue #622: Ein Nutzerwechsel widerruft den bisherigen Kanal. Laufen
    // darüber noch Operationen, geschieht das nur nach Bestätigung — ohne sie
    // kommt die Liste zurück und nichts ändert sich.
    let running = ctx.registry.ops().running_elevated(ctx.session_id);
    if !running.is_empty() {
        if !confirm_stop {
            return Ok(ElevationResultDto {
                active: true,
                target_user,
                sftp_server_path: None,
                failure: None,
                warning: None,
                running_operations: running,
                server_logging: false,
            });
        }
        stop_running_and_wait(ctx.registry, ctx.session_id).await;
    }
    enable_after_confirmation(ctx, login, override_path, requested_user, access).await
}

/// [`enable`] ohne die Prüfung auf laufende Operationen (Issue #622). Der
/// bisherige Kanal wird ohne Rücksicht auf sie widerrufen — die Widerrufs-
/// Tests (Spec 0084/0085) prüfen damit den Mechanismus selbst.
#[expect(clippy::too_many_lines, reason = "over 150 lines; split planned")]
pub(crate) async fn enable_after_confirmation(
    ctx: &ElevationContext<'_>,
    login: &str,
    override_path: Option<&str>,
    requested_user: Option<&str>,
    access: &BrowserAccess,
) -> CommandResult<ElevationResultDto> {
    let session = ctx.session;
    let target_user = requested_user
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .unwrap_or(DEFAULT_ELEVATION_USER)
        .to_string();
    if !is_valid_target_user(&target_user) {
        return failure(
            &target_user,
            None,
            ElevationFailureKind::InvalidUser,
            None,
            None,
        );
    }

    let path = match override_path {
        Some(path) if is_plausible_sftp_server_path(path) => path.to_string(),
        Some(_) => {
            return failure(
                &target_user,
                None,
                ElevationFailureKind::InvalidPath,
                None,
                None,
            );
        }
        None => match run_probe(session, &sftp_server_probe_command()).await {
            Ok(output) => match parse_sftp_server_probe(&output) {
                Some(path) => path,
                None => {
                    return failure(
                        &target_user,
                        None,
                        ElevationFailureKind::SftpServerNotFound,
                        None,
                        None,
                    );
                }
            },
            Err(detail) => {
                return failure(
                    &target_user,
                    None,
                    ElevationFailureKind::CheckFailed,
                    None,
                    Some(detail),
                );
            }
        },
    };
    let path_for_dto = Some(path.clone());

    // Spec 0067, A3: Eigentümer und Rechte von `sftp-server` und seinen
    // Elternverzeichnissen — vor dem sudo-Check, damit keine NOPASSWD-Regel
    // für ein austauschbares Binary vorgeschlagen wird. Fail closed: auch ein
    // gescheiterter oder unlesbarer Probe gilt als unsicher.
    let safety_cmd = sftp_server_safety_probe_command(&path).expect("Pfad wurde oben validiert");
    let safety = match run_probe(session, &safety_cmd).await {
        Ok(output) => parse_sftp_server_safety_probe(&output, &path),
        Err(_) => SftpServerSafety::Unsafe { path: path.clone() },
    };
    let unsafe_path = match safety {
        SftpServerSafety::Safe => None,
        SftpServerSafety::Unsafe { path } => Some(path),
    };

    // Issue #1192: first the variant with the logging arguments; only when
    // it is not allowed passwordless, the plain command (existing rules).
    let logging_cmd = sudo_check_command(&path, &target_user, true)
        .expect("Pfad und Nutzer wurden oben validiert");
    let logging_check = match run_probe(session, &logging_cmd).await {
        Ok(output) => classify_sudo_check(&output),
        Err(detail) => SudoCheck::Failed(detail),
    };
    let nopasswd_logging = logging_check == SudoCheck::Allowed;
    let check = if nopasswd_logging {
        SudoCheck::Allowed
    } else {
        let check_cmd = sudo_check_command(&path, &target_user, false)
            .expect("Pfad und Nutzer wurden oben validiert");
        match run_probe(session, &check_cmd).await {
            Ok(output) => classify_sudo_check(&output),
            Err(detail) => SudoCheck::Failed(detail),
        }
    };
    let sudoers = || sudoers_line(login, &path, &target_user);
    // Issue #1191: password path — only when `sudo -n -l` says a password is
    // needed and one is stored for the server. Nothing else switches to it.
    let stored_password = session.safety.sudo_password.get();
    let mut use_password = false;
    match check {
        SudoCheck::Allowed => {}
        SudoCheck::PasswordRequired if stored_password.is_some() => {
            use_password = true;
        }
        SudoCheck::PasswordRequired | SudoCheck::NotAllowed if unsafe_path.is_some() => {
            // Keine sudoers-Zeile für ein Binary, das ein anderer Nutzer
            // austauschen könnte.
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::SftpServerUnsafe,
                None,
                unsafe_path,
            );
        }
        SudoCheck::PasswordRequired => {
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::PasswordRequired,
                sudoers(),
                None,
            );
        }
        SudoCheck::NotAllowed => {
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::NotAllowed,
                sudoers(),
                None,
            );
        }
        SudoCheck::RequireTty => {
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::RequireTty,
                None,
                None,
            );
        }
        SudoCheck::SudoMissing => {
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::SudoMissing,
                None,
                None,
            );
        }
        SudoCheck::Failed(detail) => {
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::CheckFailed,
                None,
                Some(detail),
            );
        }
    }

    let opened = if use_password {
        open_with_password(session, &path, &target_user, stored_password.as_deref()).await
    } else {
        let command = elevated_sftp_command(&path, &target_user, nopasswd_logging)
            .expect("Pfad und Nutzer wurden oben validiert");
        let mut transport = session.transport.lock().await;
        transport
            .open_sftp_via_exec(&command)
            .await
            .map(|sftp| (sftp, nopasswd_logging))
            .map_err(OpenFailure::Other)
    };
    match opened {
        Ok((sftp, server_logging)) => {
            // Spec 0084, A2.2: Zwischen den Proben oben und diesem Eintrag
            // liegen mehrere `.await`-Punkte — die Sitzung kann inzwischen
            // getrennt worden sein. Dann wird der eben geöffnete Kanal hier
            // verworfen (sein `Drop` schließt ihn), statt als Eintrag ohne
            // Sitzung stehen zu bleiben.
            let channel = ElevatedSftp {
                target_user: target_user.clone(),
                sudo_auth: if use_password {
                    SudoAuth::Password
                } else {
                    SudoAuth::Nopasswd
                },
                server_logging,
                sftp,
            };
            let previous = ctx
                .registry
                .insert_if_session_alive(ctx.sessions, ctx.session_id, channel, access)
                .map_err(|()| app_logic::session::session_not_found())?;
            // Spec 0084, §9 (Klarstellung 2026-09-28): Wer den bisherigen
            // Kanal noch aus einem laufenden Befehl hält, darf ihn über das
            // Neu-Aktivieren hinaus nicht weiter benutzen — sonst liefe ein
            // Download, der als „root“ begonnen hat, nach dem Umschalten auf
            // `www-data` weiter als root.
            if let Some(previous) = previous {
                previous.revoke().await;
            }
            tracing::info!(
                target_user = %target_user,
                sftp_server_path = %path,
                source = "manual",
                sudo_auth = if use_password { "password" } else { "nopasswd" },
                server_logging,
                "file browser elevated rights enabled"
            );
            Ok(ElevationResultDto {
                active: true,
                target_user,
                sftp_server_path: path_for_dto,
                failure: None,
                warning: unsafe_path.map(|path| ElevationWarningDto {
                    kind: ElevationWarningKind::SftpServerUnsafe,
                    path,
                }),
                running_operations: Vec::new(),
                server_logging,
            })
        }
        Err(OpenFailure::WrongPassword) => failure(
            &target_user,
            path_for_dto,
            ElevationFailureKind::WrongSudoPassword,
            None,
            None,
        ),
        Err(OpenFailure::NotAllowed(line)) => failure(
            &target_user,
            path_for_dto,
            ElevationFailureKind::NotAllowed,
            sudoers(),
            Some(line),
        ),
        Err(OpenFailure::Other(err)) => failure(
            &target_user,
            path_for_dto,
            ElevationFailureKind::StartFailed,
            None,
            Some(err.to_string()),
        ),
    }
}

/// Why opening the elevated channel failed (Issue #1191).
enum OpenFailure {
    WrongPassword,
    NotAllowed(String),
    Other(ssh_manager_core::ssh::SshError),
}

/// Issue #1191: opens the channel with `sudo -S -k -p '<marker>'` and the
/// stored sudo password. The transport writes the password only after the
/// marker appeared on stderr; the password is not part of any text here.
/// Issue #1192: first with the logging arguments; when sudo says that command
/// is not allowed, one retry without arguments (fresh marker). Returns the
/// channel and whether it logs on the server.
async fn open_with_password(
    session: &Session,
    path: &str,
    target_user: &str,
    password: Option<&secrecy::SecretString>,
) -> Result<(Box<dyn ssh_manager_core::ssh::SftpSession>, bool), OpenFailure> {
    use ssh_manager_core::ssh::{SshError, SudoPromptError};
    let Some(password) = password else {
        return Err(OpenFailure::Other(SshError::ChannelError(
            "no sudo password stored".to_string(),
        )));
    };
    let mut transport = session.transport.lock().await;
    let mut logging = true;
    loop {
        let marker = new_sudo_prompt_marker();
        let command = elevated_sftp_password_command(path, target_user, &marker, logging)
            .expect("Pfad, Nutzer und Marker sind gültig");
        match transport
            .open_sftp_via_exec_with_sudo_password(&command, &marker, password)
            .await
        {
            Ok(sftp) => return Ok((sftp, logging)),
            Err(SudoPromptError::NotAllowed(_)) if logging => logging = false,
            Err(SudoPromptError::NotAllowed(line)) => return Err(OpenFailure::NotAllowed(line)),
            Err(SudoPromptError::NoPrompt) => {
                return Err(OpenFailure::Other(SshError::ChannelError(
                    "sudo did not ask for the password".to_string(),
                )))
            }
            Err(SudoPromptError::WrongPassword) => return Err(OpenFailure::WrongPassword),
            Err(SudoPromptError::Failed(err)) => return Err(OpenFailure::Other(err)),
        }
    }
}

/// Schaltet den erhöhten Kanal einer Sitzung aus und **widerruft** ihn.
///
/// Spec 0084, §9 (Klarstellungen 2026-09-28):
/// * Der Widerruf wartet, bis ein gerade laufender Vorgang auf dem Kanal
///   fertig ist, und nimmt den Kanal dann heraus — ein Befehl, der ihn noch
///   festhält, scheitert danach beim Zugriff, statt weiter erhöht zu
///   arbeiten.
/// * Eine **unbekannte** Sitzungskennung liefert wie bisher
///   `Err("session not found")`; „nicht aktiv“ (also `Ok`, ohne dass
///   etwas passiert) gilt nur für eine bekannte Sitzung ohne Kanal (T9).
///
/// Issue #622: Laufen noch erhöhte Operationen, passiert ohne `confirm_stop`
/// **nichts** — die Liste kommt zurück (`disabled == false`) und der Kanal
/// bleibt offen, nur für diese Operationen. Mit `confirm_stop` werden sie
/// zuerst gestoppt (und räumen ihre Teilergebnisse über den noch gültigen
/// Kanal auf), erst danach wird widerrufen.
pub(crate) async fn disable(
    sessions: &SessionManager,
    registry: &ElevatedSftpRegistry,
    session_id: SessionId,
    confirm_stop: bool,
    access: &BrowserAccess,
) -> CommandResult<ElevationDisableDto> {
    app_logic::session::require_session(sessions, session_id)?;
    let running = registry.ops().running_elevated(session_id);
    if !running.is_empty() {
        if !confirm_stop {
            return Ok(ElevationDisableDto {
                disabled: false,
                running_operations: running,
            });
        }
        stop_running_and_wait(registry, session_id).await;
    }
    if let Some(slot) = registry.take(session_id, access) {
        if slot.revoke().await {
            tracing::info!(
                source = "manual",
                reason = "manual",
                "file browser elevated rights disabled"
            );
        }
    }
    Ok(ElevationDisableDto {
        disabled: true,
        running_operations: Vec::new(),
    })
}

/// Nur für Tests: das Ausschalten ohne Bestätigungsdialog und ohne die
/// laufenden Operationen vorher zu stoppen — der reine Widerruf des Kanals,
/// wie ihn Trennen und Neu-Aktivieren auslösen. Die Widerrufs-Tests (Spec
/// 0084/0085) prüfen damit, dass ein Widerruf mitten in einem Befehl sofort
/// wirkt.
#[cfg(test)]
pub(crate) async fn disable_now(
    sessions: &SessionManager,
    registry: &ElevatedSftpRegistry,
    session_id: SessionId,
    access: &BrowserAccess,
) -> CommandResult<()> {
    app_logic::session::require_session(sessions, session_id)?;
    if let Some(slot) = registry.take(session_id, access) {
        slot.revoke().await;
    }
    Ok(())
}

/// Issue #622: bestätigtes Stoppen — Transfers enden beim nächsten Stück,
/// Rekursionen beim nächsten Eintrag; danach bleibt Zeit fürs Aufräumen
/// (Temp-Datei, Teil-Upload), das noch den Kanal braucht. Bleibt eine
/// Operation hängen, wird nach [`STOP_WAIT`] trotzdem widerrufen
/// (Sicherheit vor Aufräumen).
async fn stop_running_and_wait(registry: &ElevatedSftpRegistry, session_id: SessionId) {
    registry
        .ops()
        .stop_elevated(session_id, StopReason::Stopped);
    registry
        .ops()
        .wait_elevated_idle(session_id, STOP_WAIT)
        .await;
}

#[cfg(test)]
mod tests;
