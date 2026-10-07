//! Spec 0104 / Issue #50: eigene MCP-Sitzungen je (Server, MCP-Client).
//!
//! Bis hierhin landete eine MCP-Anfrage in einer bereits offenen, verbundenen
//! Nutzer-Sitzung desselben Servers (Spec 0028, Abschnitt 9a alt: "existiert
//! bereits ein Tab für diesen Server, wird dieser verwendet"). MCP-Ausgaben
//! liefen dadurch in den KI-Verlauf des Nutzers, der Tab wurde nach vorne
//! geholt. Diese Registry trennt das: Eine MCP-Anfrage wird ausschließlich
//! einer Sitzung zugeordnet, die **für genau diesen MCP-Client auf genau
//! diesem Server** angelegt wurde. Eine Nutzer-Sitzung ist hier nie
//! eingetragen und kann deshalb nie getroffen werden — die Trennung ist
//! keine zusätzliche Prüfung im Ausführungspfad, sondern eine Folge davon,
//! welche Sitzung `app_shell::mcp_backend` überhaupt in der Hand hat.
//!
//! Filter-Engine, Redaction, Fencing und der erzwungene Confirm-Zweig
//! (`FILTER_MCP_ORIGIN_REQUIRES_CONFIRM`) bleiben unverändert: Die
//! MCP-Sitzung ist eine ganz normale `Session` mit eigenem Transport,
//! eigener Filter-Engine und eigenem Redactor, und jede Aktion läuft weiter
//! über `orchestration::handle_mcp_action_proposed`.
//!
//! Bewusst Tauri-frei (Spec 0084), damit Zuordnung, Wiederverwendung und
//! das Ablehnen offener Bestätigungen beim Schließen ohne `AppHandle`
//! getestet sind.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use ssh_manager_core::shared::ServerId;

use crate::confirmation::ConfirmationRegistry;
use crate::dto::ActionUserDecision;
use crate::error::CommandResult;
use crate::events::{emit_mcp_action_tab_requested, ConnectionStatus, EventEmitter};
use crate::poison::lock_tolerating_poison;
use crate::session::{Session, SessionManager};
use crate::state::{ActionId, SessionId};

/// Meldung an das Frontend, wenn ein Nutzer-Kommando (Chat, Terminal) eine
/// MCP-Sitzung trifft. Die MCP-Sitzung hat keinen Nutzer-Chat und kein
/// interaktives Terminal (Spec 0104, §4) — sonst liefe Nutzer-Kontext in die
/// MCP-Sitzung, und genau das schließt Issue #50 aus.
pub const MCP_SESSION_NOT_INTERACTIVE_MESSAGE: &str =
    "Diese Sitzung gehört einem externen Tool (MCP) und nimmt keine eigenen Eingaben an.";

/// Meldung an den MCP-Client, wenn seine Sitzung in der App geschlossen
/// wurde, während seine Aktion noch auf Bestätigung wartete.
pub const MCP_SESSION_CLOSED_MESSAGE: &str =
    "Die MCP-Sitzung wurde in der App geschlossen; die Aktion wurde nicht ausgeführt. \
     Eine neue Anfrage öffnet eine neue MCP-Sitzung.";

/// Issue #68: Höchstlänge des Client-Namens in Zeichen (nicht Bytes). Der
/// Name ist frei wählbar und Teil des Sitzungsschlüssels; ohne Grenze liefe
/// beliebig langer Text in Tab-Beschriftung und OS-Benachrichtigung, und
/// jede Abweichung hinter dieser Grenze ergäbe einen eigenen Schlüssel.
pub const MCP_CLIENT_NAME_MAX_CHARS: usize = 64;

/// Issue #68: Höchstzahl offener MCP-Sitzungen je Server. Zählt jede
/// eingetragene Sitzung, auch eine abgerissene, deren Tab noch offen ist —
/// sie belegt weiter einen Tab. Schließen des Tabs gibt den Platz frei.
pub const MCP_MAX_SESSIONS_PER_SERVER: usize = 4;

/// Meldung an den MCP-Client, wenn für den Server schon
/// [`MCP_MAX_SESSIONS_PER_SERVER`] MCP-Sitzungen offen sind.
pub const MCP_SESSION_LIMIT_MESSAGE: &str =
    "Für diesen Server sind bereits 4 MCP-Sitzungen offen (Höchstzahl). \
     Schließe in der App einen MCP-Tab dieses Servers und versuche es erneut.";

/// Der Client-Name, wie ihn Schlüssel und Anzeige verwenden: getrimmt, auf
/// [`MCP_CLIENT_NAME_MAX_CHARS`] Zeichen gekürzt (an einer Zeichengrenze,
/// nie mitten in einem Multi-Byte-Zeichen), `None` wenn danach leer.
pub fn normalize_client_name(client_name: Option<&str>) -> Option<String> {
    let trimmed = client_name.map(str::trim).unwrap_or_default();
    let shortened = match trimmed.char_indices().nth(MCP_CLIENT_NAME_MAX_CHARS) {
        Some((cut, _)) => trimmed[..cut].trim_end(),
        None => trimmed,
    };
    (!shortened.is_empty()).then(|| shortened.to_string())
}

/// Schlüssel einer MCP-Sitzung: Server plus Name des MCP-Clients aus dem
/// Handshake (`clientInfo.name`, s. [`normalize_client_name`]). Clients
/// ohne Namen teilen sich je Server eine Sitzung (leerer Name) — sie sind
/// für die App nicht unterscheidbar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct McpSessionKey {
    server_id: ServerId,
    client: String,
}

impl McpSessionKey {
    pub fn new(server_id: ServerId, client_name: Option<&str>) -> Self {
        Self {
            server_id,
            client: normalize_client_name(client_name).unwrap_or_default(),
        }
    }

    pub fn server_id(&self) -> ServerId {
        self.server_id
    }

    /// `None` für einen Client ohne Namen.
    pub fn client_name(&self) -> Option<String> {
        (!self.client.is_empty()).then(|| self.client.clone())
    }
}

/// Was die App über eine MCP-Sitzung weiß — für die Tab-Beschriftung
/// ("<Client> @ <Server>") und die Sperre von Nutzer-Eingaben.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSessionInfo {
    pub server_id: ServerId,
    pub client_name: Option<String>,
}

#[derive(Default)]
struct Inner {
    /// Die aktuell für einen Schlüssel zu verwendende Sitzung.
    current: HashMap<McpSessionKey, SessionId>,
    /// Alle noch offenen MCP-Sitzungen, auch eine, deren Verbindung
    /// abgerissen ist und die deshalb nicht mehr `current` ist — ihr Tab
    /// bleibt bis zum Schließen offen und bleibt bis dahin eine MCP-Sitzung
    /// (keine Nutzer-Eingaben).
    sessions: HashMap<SessionId, McpSessionInfo>,
}

/// [`McpSessionRegistry::register`] scheitert: Für den Server sind schon
/// [`MCP_MAX_SESSIONS_PER_SERVER`] MCP-Sitzungen offen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpSessionLimitReached;

impl std::fmt::Display for McpSessionLimitReached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(MCP_SESSION_LIMIT_MESSAGE)
    }
}

impl std::error::Error for McpSessionLimitReached {}

/// Ergebnis von [`McpSessionRegistry::acquire_session`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpSessionSlot {
    /// Die verbundene Sitzung dieses Schlüssels wird weiterverwendet.
    Existing(SessionId),
    /// Eine neue Sitzung ist eingetragen und ihr Tab angekündigt; der
    /// Aufrufer baut jetzt die Verbindung auf.
    New(SessionId),
}

/// Anlege-Lock eines Schlüssels plus Zahl der Aufgaben, die es halten oder
/// darauf warten. Bei 0 wird der Eintrag entfernt (Issue #68), damit die
/// Tabelle nicht mit jedem je gesehenen Client-Namen wächst.
struct CreationLockEntry {
    lock: Arc<tokio::sync::Mutex<()>>,
    users: usize,
}

#[derive(Default)]
pub struct McpSessionRegistry {
    inner: StdMutex<Inner>,
    /// Ein Anlege-Lock je Schlüssel: Zwei gleichzeitige Anfragen desselben
    /// Clients an denselben Server dürfen nicht zwei Verbindungen aufbauen.
    /// Je Schlüssel statt global, damit ein offener Host-Key-Dialog für
    /// Server A keine Anfrage an Server B aufhält. Enthält nur Schlüssel,
    /// für die gerade eine Aufgabe das Lock hält oder darauf wartet.
    creation_locks: StdMutex<HashMap<McpSessionKey, CreationLockEntry>>,
}

/// Hält das Anlege-Lock eines Schlüssels (s.
/// [`McpSessionRegistry::lock_creation`]).
pub struct McpCreationGuard<'a> {
    // Reihenfolge ist wichtig: Felder fallen in Deklarationsreihenfolge.
    // Erst das Lock freigeben, dann abmelden — sonst könnte zwischen
    // Entfernen des Eintrags und Freigabe eine neue Anfrage ein zweites
    // Lock für denselben Schlüssel anlegen und parallel laufen.
    _held: tokio::sync::OwnedMutexGuard<()>,
    _user: CreationLockUser<'a>,
}

/// Eine Aufgabe, die das Anlege-Lock eines Schlüssels hält oder darauf
/// wartet. Meldet sich beim Fallen ab — auch wenn das Warten abgebrochen
/// wird (MCP-Anfrage verworfen) — und entfernt den Eintrag, sobald niemand
/// mehr darauf zugreift.
struct CreationLockUser<'a> {
    registry: &'a McpSessionRegistry,
    key: McpSessionKey,
}

impl Drop for CreationLockUser<'_> {
    fn drop(&mut self) {
        let mut locks = lock_tolerating_poison(&self.registry.creation_locks);
        if let Some(entry) = locks.get_mut(&self.key) {
            entry.users = entry.users.saturating_sub(1);
            if entry.users == 0 {
                locks.remove(&self.key);
            }
        }
    }
}

impl McpSessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hält das Anlege-Lock für `key`, bis der zurückgegebene Guard fällt.
    /// Aufrufer prüft unter diesem Lock zuerst [`Self::reusable_session`]
    /// und legt nur bei `None` eine neue Sitzung an.
    ///
    /// Der Eintrag für `key` existiert nur, solange eine Aufgabe das Lock
    /// hält oder darauf wartet. Ein später ankommender Aufrufer bekommt
    /// dann zwar ein neues Lock, findet die inzwischen eingetragene Sitzung
    /// aber über [`Self::reusable_session`] — Eintragen geschieht immer
    /// unter dem Lock.
    pub async fn lock_creation(&self, key: &McpSessionKey) -> McpCreationGuard<'_> {
        let (lock, user) = {
            let mut locks = lock_tolerating_poison(&self.creation_locks);
            let entry = locks
                .entry(key.clone())
                .or_insert_with(|| CreationLockEntry {
                    lock: Arc::default(),
                    users: 0,
                });
            entry.users += 1;
            (
                Arc::clone(&entry.lock),
                CreationLockUser {
                    registry: self,
                    key: key.clone(),
                },
            )
        };
        let held = lock.lock_owned().await;
        McpCreationGuard {
            _held: held,
            _user: user,
        }
    }

    /// Zahl der Schlüssel mit Anlege-Lock-Eintrag (für Tests, Issue #68).
    #[cfg(test)]
    fn creation_lock_entries(&self) -> usize {
        lock_tolerating_poison(&self.creation_locks).len()
    }

    /// Die bestehende, verbundene MCP-Sitzung für `key`, sonst `None`.
    /// Fragt ausschließlich die eigene Zuordnung ab — eine Nutzer-Sitzung
    /// desselben Servers ist hier nie eingetragen und wird deshalb nie
    /// zurückgegeben, egal ob sie verbunden ist.
    ///
    /// Ist die eingetragene Sitzung verschwunden oder nicht mehr verbunden,
    /// wird nur die `current`-Zuordnung gelöst: Die nächste Anfrage legt
    /// eine neue Sitzung an, ein noch offener alter Tab bleibt als
    /// MCP-Sitzung gekennzeichnet, bis er geschlossen wird.
    pub fn reusable_session(
        &self,
        key: &McpSessionKey,
        sessions: &SessionManager,
    ) -> Option<SessionId> {
        let mut inner = lock_tolerating_poison(&self.inner);
        let session_id = *inner.current.get(key)?;
        match sessions.get(session_id) {
            Some(session)
                if *lock_tolerating_poison(&session.status) == ConnectionStatus::Connected =>
            {
                Some(session_id)
            }
            Some(_) => {
                inner.current.remove(key);
                None
            }
            None => {
                inner.current.remove(key);
                inner.sessions.remove(&session_id);
                None
            }
        }
    }

    /// Trägt eine neu angelegte MCP-Sitzung ein — vor dem Verbindungsaufbau,
    /// damit `list_sessions()` den Tab schon während eines Host-Key-Dialogs
    /// als MCP-Tab beschriftet und Nutzer-Eingaben von Anfang an gesperrt
    /// sind.
    ///
    /// Issue #68: Sind für den Server schon
    /// [`MCP_MAX_SESSIONS_PER_SERVER`] MCP-Sitzungen offen, wird nichts
    /// eingetragen. Der Aufrufer baut dann weder eine Verbindung auf noch
    /// öffnet er einen Tab. Prüfen und Eintragen geschehen unter einem Lock,
    /// damit gleichzeitige Anfragen verschiedener Clients (verschiedene
    /// Anlege-Locks) die Grenze nicht gemeinsam überschreiten.
    pub fn register(
        &self,
        key: &McpSessionKey,
        session_id: SessionId,
    ) -> Result<(), McpSessionLimitReached> {
        let mut inner = lock_tolerating_poison(&self.inner);
        let open_on_server = inner
            .sessions
            .values()
            .filter(|info| info.server_id == key.server_id)
            .count();
        if open_on_server >= MCP_MAX_SESSIONS_PER_SERVER {
            return Err(McpSessionLimitReached);
        }
        inner.current.insert(key.clone(), session_id);
        inner.sessions.insert(
            session_id,
            McpSessionInfo {
                server_id: key.server_id,
                client_name: key.client_name(),
            },
        );
        Ok(())
    }

    /// Der Schritt vor dem Verbindungsaufbau, unter dem Anlege-Lock von
    /// `key` (der Guard ist der Nachweis): eine verbundene Sitzung des
    /// Schlüssels wiederverwenden — das gilt auch bei erreichter
    /// Höchstzahl —, sonst eine neue eintragen und ihren Tab per
    /// `mcp-action-tab-requested` ankündigen (vor einem eventuellen
    /// Host-Key-Dialog, Spec 0104, §3).
    ///
    /// Issue #68: Ist die Höchstzahl je Server erreicht, gibt es weder
    /// Eintrag noch Event, und der Aufrufer baut keine Verbindung auf.
    pub fn acquire_session(
        &self,
        _creation: &McpCreationGuard<'_>,
        key: &McpSessionKey,
        sessions: &SessionManager,
        emitter: &dyn EventEmitter,
    ) -> Result<McpSessionSlot, McpSessionLimitReached> {
        if let Some(session_id) = self.reusable_session(key, sessions) {
            return Ok(McpSessionSlot::Existing(session_id));
        }
        let session_id: SessionId = uuid::Uuid::new_v4();
        self.register(key, session_id)?;
        emit_mcp_action_tab_requested(emitter, session_id, key.server_id, key.client_name());
        Ok(McpSessionSlot::New(session_id))
    }

    /// Entfernt eine MCP-Sitzung (Tab geschlossen, Verbindungsaufbau
    /// fehlgeschlagen). `None`, wenn `session_id` keine MCP-Sitzung war.
    pub fn unregister(&self, session_id: SessionId) -> Option<McpSessionInfo> {
        let mut inner = lock_tolerating_poison(&self.inner);
        let info = inner.sessions.remove(&session_id)?;
        inner.current.retain(|_, id| *id != session_id);
        Some(info)
    }

    pub fn info(&self, session_id: SessionId) -> Option<McpSessionInfo> {
        lock_tolerating_poison(&self.inner)
            .sessions
            .get(&session_id)
            .cloned()
    }

    pub fn is_mcp_session(&self, session_id: SessionId) -> bool {
        lock_tolerating_poison(&self.inner)
            .sessions
            .contains_key(&session_id)
    }

    /// Sperre für Nutzer-Kommandos (Chat senden, Terminal öffnen): Eine
    /// MCP-Sitzung nimmt keine Nutzer-Eingaben an.
    pub fn ensure_user_session(&self, session_id: SessionId) -> CommandResult<()> {
        if self.is_mcp_session(session_id) {
            return Err(MCP_SESSION_NOT_INTERACTIVE_MESSAGE.into());
        }
        Ok(())
    }

    /// Schließen einer Sitzung: Ist sie eine MCP-Sitzung, wird sie
    /// ausgetragen und eine noch wartende Bestätigung **abgelehnt** (fail
    /// closed) — der Nutzer hat den Tab samt Dialog geschlossen, die Aktion
    /// darf danach nicht mehr genehmigt werden können und soll nicht erst
    /// am Bestätigungs-Timeout enden. Gibt zurück, ob es eine MCP-Sitzung
    /// war. `session` ist `None`, wenn sie beim Schließen nicht (mehr) im
    /// `SessionManager` stand (z. B. Schließen während des Host-Key-Dialogs).
    pub fn end_session(
        &self,
        session_id: SessionId,
        session: Option<&Session>,
        confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    ) -> bool {
        if self.unregister(session_id).is_none() {
            return false;
        }
        let pending = session.and_then(|s| *lock_tolerating_poison(&s.pending_action));
        if let Some(action_id) = pending {
            // Kann nur scheitern, wenn die Bestätigung im selben Moment
            // anderweitig aufgelöst wurde (Klick, Timeout) — dann gibt es
            // nichts mehr abzulehnen.
            if let Err(err) = confirmations.resolve(&action_id, ActionUserDecision::Deny) {
                tracing::debug!(error = %err, "pending MCP confirmation already settled on close");
            }
        }
        true
    }
}

#[cfg(test)]
mod tests;
