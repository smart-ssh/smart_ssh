//! Spec 0084, A5: Testhilfen, die Tests **beider** Crates brauchen
//! (`app-logic` selbst und `app-shell`, dessen Tests über die Crate-Grenze
//! hinweg dieselben In-Memory-Doubles/Fixtures brauchen) — deshalb hinter
//! `#[cfg(any(test, feature = "test-support"))]` statt reinem
//! `#[cfg(test)]`, sonst wären sie außerhalb dieser Crate unsichtbar.
//! `app-shell` aktiviert das Feature nur unter `[dev-dependencies]` (s.
//! `app-logic/Cargo.toml`); kein Produktivbau aktiviert es.
//!
//! Ursprünglich (vor Spec 0084) ein einziges Modul in `app-shell` für die
//! Spec-0008-Unit-Tests (`groups`, `server_credentials`, `test_connection`)
//! und seither breit wiederverwendet. **Was hier bewusst NICHT liegt**: die
//! Testhilfen für den erhöhten Dateibrowser-Kanal (`BrowserAccess`,
//! `interleave_hook`) — die bleiben in `app-shell::test_support::elevation`
//! (Spec 0084, A1: `BrowserAccess` ist nur in `commands::elevation`
//! konstruierbar; ein Nachbau hier würde genau diese Garantie unterlaufen).
//! `app-shell`s `elevation`-Fixture ruft [`session_with_transport`]
//! trotzdem hier drüben auf (cross-crate) — nicht weil `Session`s Felder
//! privat wären (sie sind `pub`), sondern damit der Bauplan für eine
//! Test-`Session` an einer einzigen Stelle gepflegt wird, statt bei jedem
//! neuen `Session`-Feld auch in `app-shell` nachgezogen werden zu müssen.

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
    /// Spec 0076, C-6 (zweite Richtung): lässt `update_server`
    /// deterministisch fehlschlagen — simuliert einen DB-Fehler **nach**
    /// dem bereits geschriebenen Schlüsselbund-Eintrag, damit der Rollback
    /// der Überführung gegen einen echten Fehler geprüft werden kann.
    pub fail_update_server: bool,
    /// Spec 0075, §6.3.9: lässt `create_server` **ab dem n-ten** Aufruf
    /// scheitern. `fail_create_server` allein reicht dort nicht: Ein Fehler
    /// schon beim ersten Insert erreicht den interessanten Zustand nie —
    /// geprüft werden soll die Rücknahme, wenn bereits Profile **und** eine
    /// Gruppe angelegt sind (§3.1.11).
    pub fail_create_server_after: Mutex<Option<usize>>,
    create_server_calls: Mutex<usize>,
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

    pub fn with_failing_update_server(mut self) -> Self {
        self.fail_update_server = true;
        self
    }

    /// Spec 0075, §6.3.9: ab dem `n`-ten `create_server`-Aufruf (1-basiert)
    /// scheitert jeder weitere.
    pub fn fail_create_server_after(&self, n: usize) {
        *self.fail_create_server_after.lock().unwrap() = Some(n);
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
        {
            let mut calls = self.create_server_calls.lock().unwrap();
            *calls += 1;
            if let Some(n) = *self.fail_create_server_after.lock().unwrap() {
                if *calls >= n {
                    return Err(ProfileError::Backend(
                        "simulierter DB-Fehler ab Aufruf n (Test)".to_string(),
                    ));
                }
            }
        }
        self.servers
            .lock()
            .unwrap()
            .insert(server.id, server.clone());
        Ok(())
    }

    async fn update_server(&self, server: &Server) -> ProfileResult<()> {
        if self.fail_update_server {
            return Err(ProfileError::Backend(
                "simulierter DB-Fehler beim Speichern des Servers".to_string(),
            ));
        }
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

/// Spec 0067: Test-Session mit frei wählbarem Transport und ohne KI — für
/// Tests des erhöhten Dateibrowser-Kanals. Ruft [`session_with_ai_and_
/// transport`] mit einem `AiProvider`, der nur `Done` liefert. Liegt hier
/// (statt in `app-shell`) als der eine kanonische Bauplan für eine
/// Test-`Session` — jedes Feld ist zwar `pub` und ließe sich auch von
/// `app-shell` aus einzeln literal bauen, aber nur an dieser einen Stelle
/// gepflegt zu werden erspart, ihn bei jedem neuen `Session`-Feld an
/// mehreren Stellen nachzuziehen. `app-shell::test_support::elevation::
/// fixture` ruft diese Funktion cross-crate auf.
pub fn session_with_transport(
    transport: Box<dyn ssh_manager_core::ssh::SshTransport>,
) -> crate::session::Session {
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

    session_with_ai_and_transport(NoAi, transport)
}

/// Wie [`session_with_transport`], aber mit frei wählbarem `AiProvider` —
/// für Tests, die eine bestimmte, vom Provider vorgeschlagene Aktion
/// brauchen (z. B. Spec 0084, T5/T5b: KI-/MCP-Dateizugriffe dürfen den
/// erhöhten Kanal nie erreichen, dafür muss die KI erst `ReadRemoteFile`/
/// `WriteRemoteFile` vorschlagen). `app-shell` reicht dafür z. B. seinen
/// eigenen `test_support::MockAiProvider` (Vec-von-Events) herein.
pub fn session_with_ai_and_transport(
    ai_provider: impl ssh_manager_core::ai::AiProvider + 'static,
    transport: Box<dyn ssh_manager_core::ssh::SshTransport>,
) -> crate::session::Session {
    use std::sync::Arc;
    use tokio::sync::Mutex as AsyncMutex;

    crate::session::Session::new(crate::session::SessionParts {
        transport: crate::session::SessionTransport::new(transport),
        ai_provider: Box::new(ai_provider),
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
        auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
        auto_continue_stop_notify: tokio::sync::Notify::new(),
        chat_turn: std::sync::Mutex::new(crate::session::ChatTurnState::default()),
        risk_second_opinion_provider: None,
        risk_second_opinion_budget: None,
        // Spec 0092, §5: standardmäßig „an", wie in der App.
        red_risk_always_confirm: true,
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
    })
}

/// Aufzeichnung der `tracing`-Ereignisse dieses Testbinaries.
///
/// **Warum geteilt und nicht je Testmodul eigen** (Spec 0077, T-6c): Ein
/// globaler `tracing`-Default lässt sich pro Prozess nur **einmal** setzen.
/// Installierte jedes Testmodul seinen eigenen, gewänne nur das zuerst
/// laufende; alle anderen Module schrieben danach in den thread-lokalen
/// Puffer des fremden Subscribers und sähen ihre eigenen Zeilen nie — je
/// nach Testreihenfolge mal so, mal so. Dieselbe Lehre steht in
/// `ai_providers::test_support`. Deshalb: **ein** Subscriber für das ganze
/// Binary, von `orchestration` und `filter_rules` gemeinsam genutzt.
///
/// **Warum global statt `with_default`**: `tracing-core` cacht das
/// Callsite-Interesse prozessweit. Trifft ein Thread ohne Subscriber eine
/// Log-Stelle zuerst, kann sie als „niemand interessiert" gecacht werden,
/// und ein späteres `with_default` auf einem anderen Thread gewinnt dieses
/// Wettrennen nicht zuverlässig zurück (beobachtet in `orchestration`:
/// etwa jeder dritte Lauf verlor den Eintrag).
///
/// **Aufzeichnung auf `TRACE`, Filter beim Lesen** (Spec 0094, §7): Hier
/// stand „Kein Level-Filter" — das war falsch. Ohne `with_max_level` gilt der
/// Standard von `tracing-subscriber`, und der ist `INFO`; `debug`- und
/// `trace`-Ereignisse landeten also nie im Puffer. Spec 0094 braucht beide
/// Richtungen: „auf `info` steht der Inhalt nicht" **und** „auf `debug` steht
/// er". Deshalb jetzt ausdrücklich `TRACE` beim Aufzeichnen und ein Filter
/// beim Lesen — [`recorded_error_lines`] für Spec 0077,
/// [`recorded_lines_at_info_or_above`] und [`recorded_debug_lines`] für
/// Spec 0094.
pub mod log_capture {
    thread_local! {
        /// Je Thread ein eigener Puffer — sicher unter paralleler
        /// Testausführung, da jeder Test-Thread nur seine eigenen Zeilen
        /// sieht.
        static BUFFER: std::cell::RefCell<Vec<u8>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    #[derive(Clone, Default)]
    pub struct ThreadLocalTestWriter;

    impl std::io::Write for ThreadLocalTestWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            BUFFER.with(|b| b.borrow_mut().extend_from_slice(buf));
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ThreadLocalTestWriter {
        type Writer = ThreadLocalTestWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Installiert den Subscriber einmal pro Testprozess und leert den
    /// Puffer dieses Threads. Jeder aufzeichnende Test ruft das als Erstes.
    pub fn start_recording() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_max_level(tracing::level_filters::LevelFilter::TRACE)
                .with_writer(ThreadLocalTestWriter)
                .finish();
            // `let _ =`: schlägt nur fehl, wenn schon ein globaler Default
            // gesetzt ist — dank `Once` wäre das bereits dieser hier.
            let _ = tracing::subscriber::set_global_default(subscriber);
        });
        BUFFER.with(|b| b.borrow_mut().clear());
    }

    /// Der rohe aufgezeichnete Text dieses Threads.
    pub fn recorded_text() -> String {
        BUFFER.with(|b| String::from_utf8(b.borrow().clone()).expect("Log ist kein UTF-8"))
    }

    /// Nur die Ereignisse auf ERROR-Ebene, eine JSON-Zeile je Ereignis
    /// (Spec 0077, T-6c/T-A10: Aussagen über das ERROR-Log dürfen nicht
    /// versehentlich ein INFO-Ereignis mitlesen).
    pub fn recorded_error_lines() -> Vec<String> {
        lines_at_levels(&["ERROR"])
    }

    /// Spec 0094, A1: die Zeilen, über die A1 eine Aussage macht — `info`,
    /// `warn`, `error`. `debug`/`trace` bleiben draußen, denn dort ist
    /// Inhalt laut A2 ausdrücklich erlaubt.
    pub fn recorded_lines_at_info_or_above() -> Vec<String> {
        lines_at_levels(&["INFO", "WARN", "ERROR"])
    }

    /// Spec 0094, A2: die `debug`-Zeilen, die den Inhalt tragen.
    pub fn recorded_debug_lines() -> Vec<String> {
        lines_at_levels(&["DEBUG"])
    }

    /// Filtert nach dem `level`-Feld der JSON-Zeile. Bewusst wörtlich
    /// gesucht statt die Zeile zu parsen — und mit einer Zusicherung davor,
    /// dass jede aufgezeichnete Zeile ein erkennbares Level trägt: eine
    /// Zeile, die der Filter nicht einordnen kann, würde bei einer
    /// Abwesenheits-Aussage („steht nicht auf `info`") sonst stillschweigend
    /// zu einem falschen Grün führen.
    fn lines_at_levels(levels: &[&str]) -> Vec<String> {
        let text = recorded_text();
        let lines: Vec<&str> = text.lines().collect();
        for line in &lines {
            assert!(
                ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"]
                    .iter()
                    .any(|l| line.contains(&format!("\"level\":\"{l}\""))),
                "Log-Zeile ohne erkennbares level-Feld — der Filter würde sie \
                 stillschweigend übergehen: {line}"
            );
        }
        lines
            .into_iter()
            .filter(|line| {
                levels
                    .iter()
                    .any(|l| line.contains(&format!("\"level\":\"{l}\"")))
            })
            .map(str::to_string)
            .collect()
    }

    #[cfg(test)]
    mod capture_self_check {
        /// Spec 0094, §7: dieselbe Selbstprüfung wie in
        /// `ssh_manager_core::filter::tests` — die Aussagen von T6/T9/T10
        /// sind nur belastbar, wenn der Mitschnitt `debug` und `info`
        /// wirklich sieht und beim Lesen auseinanderhält. Vor Spec 0094
        /// hätte dieser Test die `debug`-Hälfte nicht bestanden.
        #[test]
        fn test_log_capture_records_debug_and_info_separately() {
            super::start_recording();

            tracing::debug!(marker = "nur-debug-0094", "capture self-check (debug)");
            tracing::info!(marker = "nur-info-0094", "capture self-check (info)");

            let debug_lines = super::recorded_debug_lines();
            let info_or_above = super::recorded_lines_at_info_or_above();
            assert!(
                debug_lines.iter().any(|l| l.contains("nur-debug-0094")),
                "debug-Ereignis muss aufgezeichnet werden: {debug_lines:?}"
            );
            assert!(
                !debug_lines.iter().any(|l| l.contains("nur-info-0094")),
                "info-Ereignis darf nicht als debug-Zeile gelesen werden: {debug_lines:?}"
            );
            assert!(
                info_or_above.iter().any(|l| l.contains("nur-info-0094")),
                "info-Ereignis muss aufgezeichnet werden: {info_or_above:?}"
            );
            assert!(
                !info_or_above.iter().any(|l| l.contains("nur-debug-0094")),
                "debug-Ereignis darf nicht als info-Zeile gelesen werden: {info_or_above:?}"
            );
        }
    }
}
