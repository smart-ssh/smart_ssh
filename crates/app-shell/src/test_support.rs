//! Gemeinsame Test-Doubles für die Spec-0008-Unit-Tests (`groups`,
//! `server_credentials`, `test_connection`) — nur unter `#[cfg(test)]`
//! eingebunden (s. `lib.rs`). Ein geteiltes Modul statt einer
//! Neuimplementierung von `ProfileStore`/`CredentialStore` pro Testdatei:
//! alle drei brauchen dieselbe vollständige In-Memory-Semantik (u. a.
//! einen echten `get_group`-Lookup, damit `ProfileStore::group_chain`s
//! Default-Implementierung tatsächlich funktioniert), Duplikation hätte
//! hier keinen Isolationsvorteil gebracht wie bei den bewusst getrennten
//! Mocks in anderen Crates.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use secrecy::SecretString;

use ssh_manager_core::profiles::{
    CredentialError, CredentialRef, CredentialResult, CredentialStore, Group, GroupId,
    NoteRevision, NoteTarget, ProfileError, ProfileResult, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

#[derive(Default)]
pub struct InMemoryProfileStore {
    pub groups: Mutex<HashMap<GroupId, Group>>,
    pub servers: Mutex<HashMap<ServerId, Server>>,
    pub note_revisions: Mutex<Vec<NoteRevision>>,
    /// Spec 0047, Fund A2: lässt `create_server` deterministisch
    /// fehlschlagen (simuliert einen DB-Fehler NACH bereits geschriebenen
    /// Keychain-Secrets), damit der Keychain-Rollback-Pfad in
    /// `servers::create_server` gegen einen echten Fehler getestet werden
    /// kann, statt nur den Erfolgsfall abzudecken.
    pub fail_create_server: bool,
}

impl InMemoryProfileStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_group(self, group: Group) -> Self {
        self.groups.lock().unwrap().insert(group.id, group);
        self
    }

    pub fn with_server(self, server: Server) -> Self {
        self.servers.lock().unwrap().insert(server.id, server);
        self
    }

    pub fn with_failing_create_server(mut self) -> Self {
        self.fail_create_server = true;
        self
    }
}

#[async_trait]
impl ProfileStore for InMemoryProfileStore {
    async fn get_server(&self, id: &ServerId) -> ProfileResult<Server> {
        self.servers
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or(ProfileError::ServerNotFound(*id))
    }

    async fn get_group(&self, id: &GroupId) -> ProfileResult<Group> {
        self.groups
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or(ProfileError::GroupNotFound(*id))
    }

    async fn list_servers(&self) -> ProfileResult<Vec<Server>> {
        Ok(self.servers.lock().unwrap().values().cloned().collect())
    }

    async fn list_groups(&self) -> ProfileResult<Vec<Group>> {
        Ok(self.groups.lock().unwrap().values().cloned().collect())
    }

    async fn create_group(&self, group: &Group) -> ProfileResult<()> {
        self.groups.lock().unwrap().insert(group.id, group.clone());
        Ok(())
    }

    async fn update_group(&self, group: &Group) -> ProfileResult<()> {
        let mut groups = self.groups.lock().unwrap();
        if !groups.contains_key(&group.id) {
            return Err(ProfileError::GroupNotFound(group.id));
        }
        groups.insert(group.id, group.clone());
        Ok(())
    }

    async fn delete_group(&self, id: &GroupId) -> ProfileResult<()> {
        // Bildet CASCADE/SET NULL grob nach, damit Tests gegen
        // `compute_delete_group_result` auch das tatsächliche Löschen
        // verifizieren können (nicht nur die Vorschau).
        let mut groups = self.groups.lock().unwrap();
        if groups.remove(id).is_none() {
            return Err(ProfileError::GroupNotFound(*id));
        }
        let descendant_ids: Vec<GroupId> = {
            let mut affected = vec![*id];
            let mut result = Vec::new();
            let mut i = 0;
            while i < affected.len() {
                let current = affected[i];
                i += 1;
                for g in groups.values() {
                    if g.parent_id == Some(current) {
                        result.push(g.id);
                        affected.push(g.id);
                    }
                }
            }
            result
        };
        for gid in &descendant_ids {
            groups.remove(gid);
        }
        drop(groups);

        let mut servers = self.servers.lock().unwrap();
        for server in servers.values_mut() {
            if server.group_id == Some(*id)
                || server.group_id.is_some_and(|g| descendant_ids.contains(&g))
            {
                server.group_id = None;
            }
        }
        Ok(())
    }

    async fn create_server(&self, server: &Server) -> ProfileResult<()> {
        if self.fail_create_server {
            return Err(ProfileError::Backend(
                "simulierter DB-Fehler (Test)".to_string(),
            ));
        }
        self.servers
            .lock()
            .unwrap()
            .insert(server.id, server.clone());
        Ok(())
    }

    async fn update_server(&self, server: &Server) -> ProfileResult<()> {
        let mut servers = self.servers.lock().unwrap();
        if !servers.contains_key(&server.id) {
            return Err(ProfileError::ServerNotFound(server.id));
        }
        servers.insert(server.id, server.clone());
        Ok(())
    }

    async fn delete_server(&self, id: &ServerId) -> ProfileResult<()> {
        // Spec 0046, Fund 1: bildet `ON DELETE SET NULL` auf `jump_host`
        // grob nach, damit Tests gegen `compute_delete_server_result` auch
        // das tatsächliche Löschen verifizieren können (nicht nur die
        // Vorschau) — analog zum `group_id`-Nachbau in `delete_group` oben.
        let mut servers = self.servers.lock().unwrap();
        if servers.remove(id).is_none() {
            return Err(ProfileError::ServerNotFound(*id));
        }
        for server in servers.values_mut() {
            if server.jump_host == Some(*id) {
                server.jump_host = None;
            }
        }
        Ok(())
    }

    async fn record_note_revision(&self, revision: &NoteRevision) -> ProfileResult<()> {
        match revision.target {
            NoteTarget::Server(id) => {
                let mut servers = self.servers.lock().unwrap();
                let server = servers
                    .get_mut(&id)
                    .ok_or(ProfileError::ServerNotFound(id))?;
                server.notes = revision.content.clone();
                server.updated_at = revision.created_at;
            }
            NoteTarget::Group(id) => {
                let mut groups = self.groups.lock().unwrap();
                let group = groups.get_mut(&id).ok_or(ProfileError::GroupNotFound(id))?;
                group.notes = revision.content.clone();
                group.updated_at = revision.created_at;
            }
        }
        self.note_revisions.lock().unwrap().push(revision.clone());
        Ok(())
    }

    async fn list_note_revisions(&self, target: NoteTarget) -> ProfileResult<Vec<NoteRevision>> {
        Ok(self
            .note_revisions
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.target == target)
            .cloned()
            .collect())
    }
}

#[derive(Default)]
pub struct InMemoryCredentialStore {
    pub secrets: Mutex<HashMap<String, SecretString>>,
    /// Spec 0022, Abschnitt 3: Anzahl der `get()`-Aufrufe, für Tests, die
    /// nachweisen sollen, dass ein Credential über mehrere Operationen
    /// hinweg (mehrere `send_chat_message`-/`execute()`-Aufrufe) nur
    /// einmalig abgerufen und danach aus dem In-Memory-Cache (`Session`-
    /// Feld/`AiProvider`-Instanz) bedient wird, statt erneut den Store zu
    /// befragen.
    get_calls: Mutex<usize>,
    /// Spec 0047, Fund A2: lässt `set()` für jeden Ref fehlschlagen, dessen
    /// Slot-Suffix (z. B. `:sudo_password`) hierauf passt — Suffix statt
    /// exaktem Ref, weil `create_server` seine `ServerId` intern frisch
    /// erzeugt (Test kennt die konkrete ID vorab nicht). Simuliert einen
    /// Keychain-Fehler NACH bereits erfolgreich geschriebenen anderen
    /// Slots (z. B. das Sudo-Passwort nach einer bereits gespeicherten
    /// Auth-Methode) — für den Rollback-Test in `servers::create_server`.
    fail_set_for_slot_suffix: Option<String>,
    /// Spec 0071, X4: lässt `get()` mit `CredentialError::Backend`
    /// fehlschlagen, **obwohl** der Wert hinterlegt ist — der Fall "der
    /// Schlüsselbund kann nicht antworten", der sich von "kein Eintrag
    /// vorhanden" unterscheiden muss.
    fail_get_with_backend: bool,
    /// Spec 0071, A17: lässt `delete()` mit `CredentialError::Backend`
    /// fehlschlagen — der Fall „der Nutzer fordert das Entfernen an, der
    /// Schlüsselbund kann es nicht ausführen". Der Wert bleibt dabei
    /// absichtlich gespeichert, damit ein Test nachweisen kann, dass das
    /// Secret tatsächlich zurückbleibt.
    fail_delete_with_backend: bool,
}

impl InMemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_secret(self, r: &CredentialRef, value: &str) -> Self {
        self.secrets.lock().unwrap().insert(
            r.as_str().to_string(),
            SecretString::from(value.to_string()),
        );
        self
    }

    /// `slot`, z. B. `"sudo_password"` — matcht jeden Ref, dessen letztes
    /// `:`-getrenntes Segment gleich `slot` ist (s. `credential_ref` in
    /// `server_credentials.rs`: `"server:{id}:{slot}"`).
    pub fn with_failing_set_for_slot(mut self, slot: &str) -> Self {
        self.fail_set_for_slot_suffix = Some(format!(":{slot}"));
        self
    }

    /// Spec 0071, X4: simuliert einen nicht erreichbaren Schlüsselbund beim
    /// **Lesen**. Bewusst unabhängig von `with_secret`, damit sich der
    /// kritische Fall bauen lässt: Wert ist hinterlegt, der Store kann es
    /// aber nicht sagen.
    pub fn with_failing_get(mut self) -> Self {
        self.fail_get_with_backend = true;
        self
    }

    /// Spec 0071, A17: simuliert einen nicht erreichbaren Schlüsselbund
    /// beim **Löschen**.
    pub fn with_failing_delete(mut self) -> Self {
        self.fail_delete_with_backend = true;
        self
    }

    pub fn get_calls(&self) -> usize {
        *self.get_calls.lock().unwrap()
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        *self.get_calls.lock().unwrap() += 1;
        if self.fail_get_with_backend {
            return Err(CredentialError::Backend(
                "simulierter Keychain-Lesefehler (Test)".to_string(),
            ));
        }
        self.secrets
            .lock()
            .unwrap()
            .get(r.as_str())
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(r.clone()))
    }

    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        if self
            .fail_set_for_slot_suffix
            .as_deref()
            .is_some_and(|suffix| r.as_str().ends_with(suffix))
        {
            return Err(CredentialError::Backend(
                "simulierter Keychain-Fehler (Test)".to_string(),
            ));
        }
        self.secrets
            .lock()
            .unwrap()
            .insert(r.as_str().to_string(), value);
        Ok(())
    }

    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        if self.fail_delete_with_backend {
            // Absichtlich **ohne** `remove`: Das Secret bleibt stehen, so
            // wie es ein echter Schlüsselbund täte, der den Löschauftrag
            // nicht ausführen konnte.
            return Err(CredentialError::Backend(
                "simulierter Keychain-Löschfehler (Test)".to_string(),
            ));
        }
        self.secrets.lock().unwrap().remove(r.as_str());
        Ok(())
    }
}

/// Spec 0050, Fund 3: fest verdrahteter Event-Stream, unabhängig vom
/// übergebenen `SessionContext` — reicht für
/// `commands::classify_credential_test_result`, das nur das **erste**
/// Event auswertet (s. dortiger Doc-Kommentar). `ssh_manager_core::ai`
/// hat mit `MockAiProvider` (`crates/core/src/ai/tests.rs`) bereits ein
/// Äquivalent, das aber modul-privat ist (nur für Cores eigene Tests
/// gedacht) — dieselbe, hier lokal wiederholte Minimal-Implementierung
/// statt eines öffentlichen Exports nur für einen einzigen Testfall in
/// einer anderen Crate.
pub struct MockAiProvider {
    events: Vec<ssh_manager_core::ai::AiEvent>,
}

impl MockAiProvider {
    pub fn new(events: Vec<ssh_manager_core::ai::AiEvent>) -> Self {
        Self { events }
    }
}

impl ssh_manager_core::ai::AiProvider for MockAiProvider {
    fn send(
        &self,
        _context: ssh_manager_core::ai::SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = ssh_manager_core::ai::AiEvent> + Send>> {
        Box::pin(futures::stream::iter(self.events.clone()))
    }
}

/// Spec 0067: Test-Session mit frei wählbarem Transport und ohne KI — für
/// Tests des erhöhten Dateibrowser-Kanals.
pub fn session_with_transport(
    transport: Box<dyn ssh_manager_core::ssh::SshTransport>,
) -> crate::session::Session {
    use std::sync::Arc;
    use tokio::sync::Mutex as AsyncMutex;

    struct NoAi;
    impl ssh_manager_core::ai::AiProvider for NoAi {
        fn send(
            &self,
            _context: ssh_manager_core::ai::SessionContext,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = ssh_manager_core::ai::AiEvent> + Send>>
        {
            Box::pin(futures::stream::iter(vec![
                ssh_manager_core::ai::AiEvent::Done,
            ]))
        }
    }

    crate::session::Session {
        transport: AsyncMutex::new(transport),
        ai_provider: Box::new(NoAi),
        ai_provider_budget: Arc::new(ai_providers::ProviderBudgetGuard::new()),
        context: AsyncMutex::new(ssh_manager_core::ai::SessionContext {
            system_context: String::new(),
            history: Vec::new(),
            available_actions: ssh_manager_core::ai::default_action_schemas(),
            max_tokens_hint: None,
        }),
        filter_engine: Box::new(ssh_manager_core::filter::FilterEngine::new(
            crate::policy::NoRulesPolicyStore,
        )),
        server_id: ServerId::new(),
        tags: Vec::new(),
        terminal: std::sync::Mutex::new(None),
        redactor: Box::new(ssh_manager_core::ai::DefaultOutputRedactor::new()),
        ai_provider_label: "test-provider".to_string(),
        ai_model: "test-model".to_string(),
        system_context_parts: AsyncMutex::new(crate::compaction::SystemContextParts::default()),
        model_context_window_tokens: usize::MAX / 1_000,
        summary: AsyncMutex::new(None),
        mcp_origin_flags: std::sync::Mutex::new(Vec::new()),
        sudo_password: None,
        status: std::sync::Mutex::new(crate::events::ConnectionStatus::Connected),
        pending_action: std::sync::Mutex::new(None),
        sftp: AsyncMutex::new(None),
        elevated_sftp: crate::elevated_sftp::ElevatedSftpSlot::new(),
        auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
        auto_continue_stop_notify: tokio::sync::Notify::new(),
        chat_turn: std::sync::Mutex::new(crate::session::ChatTurnState::default()),
        risk_second_opinion_provider: None,
        risk_second_opinion_budget: None,
        running_command_cancellations: Arc::new(crate::confirmation::ConfirmationRegistry::new()),
        untrusted_content_ingested: std::sync::atomic::AtomicBool::new(false),
        post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
        injection_check_provider: None,
        injection_check_budget: None,
        injection_suspected: std::sync::atomic::AtomicBool::new(false),
        chat_session_store: None,
        ledger_store: None,
        chat_session_id: AsyncMutex::new(None),
        ai_request_paced_at: AsyncMutex::new(None),
    }
}
