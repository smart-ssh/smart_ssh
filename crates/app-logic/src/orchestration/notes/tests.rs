//! Tests für Notiz-Vorschlag beim Verbindungsende, Notiz-Kürzungsdialog,
//! KI-generierte Dokumente, Regel-Schnellvorschlag, Ziel-Auflösung und
//! "In Notiz übernehmen" — Spec 0083: reine Verschiebung aus
//! `orchestration::tests`, keine Verhaltensänderung.

use async_trait::async_trait;
use tokio::sync::Mutex as AsyncMutex;

use ssh_manager_core::ai::{AiError, AiEvent, DefaultOutputRedactor};
use ssh_manager_core::filter::FilterEngine;
use ssh_manager_core::profiles::{Group, GroupId, NoteRevision, ProfileResult, Server};
use ssh_manager_core::shared::ServerId;

use crate::dto::ActionUserDecision;
use crate::events::TestEmitter;
use crate::orchestration::run_chat_turn;
use crate::state::ActionId;

use super::super::test_support::*;
use super::*;

// --- Spec 0010: automatischer Notiz-Vorschlag beim Beenden -----------

fn command_result_message() -> ChatMessage {
    ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::CommandResult {
            command: "uptime".to_string(),
            output: output("up 3 days"),
            cancelled: false,
        },
    }
}

#[tokio::test]
async fn test_disconnect_suggestion_skipped_without_executed_command() {
    let provider = MockAiProvider::new(vec![AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let suggested = suggest_note_update_on_disconnect(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    assert!(
        contexts.lock().unwrap().is_empty(),
        "ohne ausgeführtes Kommando darf gar kein KI-Aufruf stattfinden (spart API-Kosten)"
    );
    assert!(emitter.events.lock().unwrap().is_empty());
    assert!(
        !suggested,
        "Rückgabewert muss false sein — Etappe 4/`should_suggest_note_shrink` verlässt sich \
         darauf, um zu entscheiden, ob der Kürzungs-Vorschlag noch laufen darf"
    );
}

#[tokio::test]
async fn test_disconnect_suggestion_calls_ai_with_restricted_actions_when_command_was_executed() {
    let provider = MockAiProvider::new(vec![AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    session
        .context
        .lock()
        .await
        .history
        .push(command_result_message());
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    suggest_note_update_on_disconnect(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let recorded = contexts.lock().unwrap();
    assert_eq!(
        recorded.len(),
        1,
        "genau ein KI-Aufruf, wenn die Schwelle erreicht ist"
    );
    assert_eq!(
        recorded[0].available_actions.len(),
        1,
        "keine SuggestCommand-Schemas anbieten (Spec Abschnitt 2, Punkt 3)"
    );
    assert_eq!(recorded[0].available_actions[0].name, "propose_note_update");
    assert!(
        recorded[0]
            .history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("Notiz"))),
        "die Abschluss-Instruktion muss im an die KI gesendeten Kontext stehen"
    );
}

/// spec-reviewer-Fund (Review dieses Schritts): schrumpft die
/// Sende-Kompaktierung (Spec 0057 §3.2, Schritt 3) die Notiz im
/// System-Prompt, darf `suggest_note_update_on_disconnect` KEINEN
/// Vorschlag einholen — die KI sähe sonst nur die gekürzte Fassung,
/// obwohl ihr Vorschlag laut `AiAction::ProposeNoteUpdate` die
/// gespeicherte Notiz VOLLSTÄNDIG ersetzen würde (Verlustrisiko für den
/// weggekürzten Teil).
#[tokio::test]
async fn test_disconnect_suggestion_skipped_when_compaction_shortens_the_note() {
    let provider = MockAiProvider::new(vec![AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let mut session = session_with_ai_provider(provider, MockSshTransport::default());
    session
        .context
        .lock()
        .await
        .history
        .push(command_result_message());
    // Winziges Fenster + große Notiz erzwingt Schritt 3 (Notiz-Kürzung).
    session.parts_mut_for_tests().model_context_window_tokens = 2_000;
    let parts = crate::compaction::SystemContextParts {
        base: "Basis".to_string(),
        note_sections: vec![("Server \"web-01\"".to_string(), "n".repeat(50_000))],
    };
    {
        let mut ctx = session.context.lock().await;
        ctx.system_context = parts.assemble();
    }
    session.parts_mut_for_tests().system_context_parts = AsyncMutex::new(parts);

    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let suggested = suggest_note_update_on_disconnect(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    assert!(
        contexts.lock().unwrap().is_empty(),
        "kein KI-Aufruf, sobald die Kompaktierung die Notiz für den Versand gekürzt hat"
    );
    assert!(
        !suggested,
        "Rückgabewert muss false sein, s. Etappe-4-Kommentar oben"
    );
}

#[tokio::test]
async fn test_disconnect_suggestion_no_event_when_ai_proposes_nothing() {
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default(),
    );
    session
        .context
        .lock()
        .await
        .history
        .push(command_result_message());
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let suggested = suggest_note_update_on_disconnect(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    assert!(
        emitter.events.lock().unwrap().is_empty(),
        "kein ActionProposed -> kein Event, kein Fehler (erwarteter Regelfall)"
    );
    assert!(
        !suggested,
        "Rückgabewert muss false sein, s. Etappe-4-Kommentar oben"
    );
}

#[tokio::test]
async fn test_disconnect_suggestion_emits_event_and_accept_persists_revision() {
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![
            AiEvent::ActionProposed(AiAction::ProposeNoteUpdate {
                target: NoteTargetSelector::CurrentServer,
                new_content: "Neuer Kontext nach der Sitzung".to_string(),
            }),
            AiEvent::Done,
        ]),
        MockSshTransport::default(),
    );
    let expected_server_id = session.server_id;
    session
        .context
        .lock()
        .await
        .history
        .push(command_result_message());
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let flow = suggest_note_update_on_disconnect(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "note-update-suggested")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Approve)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    let (suggested, ()) = tokio::join!(flow, responder);
    assert!(
        suggested,
        "Rückgabewert muss true sein — ein Vorschlag wurde tatsächlich emittiert"
    );

    let events = emitter.events.lock().unwrap().clone();
    let event_names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        event_names,
        vec!["note-update-suggested", "chat-action-result"]
    );

    let (_, suggested_payload) = &events[0];
    assert_eq!(suggested_payload["sessionId"], session_id.to_string());

    let revisions = profile_store.note_revisions.lock().unwrap();
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].content, "Neuer Kontext nach der Sitzung");
    assert_eq!(
        revisions[0].target,
        NoteTarget::Server(expected_server_id),
        "CurrentServer muss auf session.server_id auflösen"
    );
    assert_eq!(
        revisions[0].edited_by,
        NoteEditor::Ai {
            provider: "test-provider".to_string(),
            model: "test-model".to_string(),
        }
    );
}

/// Spec 0023, Abschnitt 3, letzter Satz vor Punkt 4: `note-update-
/// suggested` (die app-weite, tab-unabhängige Disconnect-Benachrichtigung,
/// Spec 0010 Abschnitt 2, Punkt 6) braucht `targetName` besonders
/// dringend — der Nutzer hat beim Empfang womöglich einen ganz anderen
/// Server offen als den, für den der Vorschlag gilt.
#[tokio::test]
async fn test_disconnect_suggestion_note_update_suggested_includes_target_name() {
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![
            AiEvent::ActionProposed(AiAction::ProposeNoteUpdate {
                target: NoteTargetSelector::CurrentServer,
                new_content: "Neuer Kontext".to_string(),
            }),
            AiEvent::Done,
        ]),
        MockSshTransport::default(),
    );
    session
        .context
        .lock()
        .await
        .history
        .push(command_result_message());
    let server_id = session.server_id;

    let now = chrono::Utc::now();
    let server = Server {
        id: server_id,
        name: "Produktions-Proxy".to_string(),
        host: "proxy.example.invalid".to_string(),
        port: 22,
        username: "deploy".to_string(),
        group_id: None,
        tags: Vec::new(),
        auth: ssh_manager_core::profiles::AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        created_at: now,
        updated_at: now,
    };
    let profile_store = crate::test_support::InMemoryProfileStore::new().with_server(server);
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let flow = suggest_note_update_on_disconnect(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "note-update-suggested")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Deny)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::join!(flow, responder);

    let events = emitter.events.lock().unwrap().clone();
    let (name, suggested_payload) = &events[0];
    assert_eq!(name, "note-update-suggested");
    assert_eq!(
        suggested_payload["targetName"],
        serde_json::json!("Produktions-Proxy")
    );
}

// --- Spec 0057, §4.2 (Etappe 4): Sitzungsende-Notiz-Kürzungs-Dialog ----

fn server_with_notes(id: ServerId, notes: &str) -> Server {
    let now = chrono::Utc::now();
    Server {
        id,
        name: "Test-Server".to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "deploy".to_string(),
        group_id: None,
        tags: Vec::new(),
        auth: ssh_manager_core::profiles::AuthMethod::Agent,
        notes: notes.to_string(),
        jump_host: None,
        post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        created_at: now,
        updated_at: now,
    }
}

/// spec-reviewer-Fund (Review dieses Schritts): Test-Double, das
/// `record_note_revision` unbedingt fehlschlagen lässt (delegiert
/// ansonsten vollständig an ein echtes `test_support::
/// InMemoryProfileStore`) — deckt den zuvor stillen Fehlerpfad ab, wenn
/// die Persistenz NACH einer Nutzer-Zustimmung fehlschlägt.
#[derive(Default)]
struct FailingRecordProfileStore {
    inner: crate::test_support::InMemoryProfileStore,
}

impl FailingRecordProfileStore {
    fn with_server(self, server: Server) -> Self {
        Self {
            inner: self.inner.with_server(server),
        }
    }
}

#[async_trait]
impl ProfileStore for FailingRecordProfileStore {
    async fn get_server(&self, id: &ServerId) -> ProfileResult<Server> {
        self.inner.get_server(id).await
    }
    async fn get_group(&self, id: &GroupId) -> ProfileResult<Group> {
        self.inner.get_group(id).await
    }
    async fn list_servers(&self) -> ProfileResult<Vec<Server>> {
        self.inner.list_servers().await
    }
    async fn list_groups(&self) -> ProfileResult<Vec<Group>> {
        self.inner.list_groups().await
    }
    async fn create_group(&self, group: &Group) -> ProfileResult<()> {
        self.inner.create_group(group).await
    }
    async fn update_group(&self, group: &Group) -> ProfileResult<()> {
        self.inner.update_group(group).await
    }
    async fn delete_group(&self, id: &GroupId) -> ProfileResult<()> {
        self.inner.delete_group(id).await
    }
    async fn create_server(&self, server: &Server) -> ProfileResult<()> {
        self.inner.create_server(server).await
    }
    async fn update_server(&self, server: &Server) -> ProfileResult<()> {
        self.inner.update_server(server).await
    }
    async fn delete_server(&self, id: &ServerId) -> ProfileResult<()> {
        self.inner.delete_server(id).await
    }
    async fn record_note_revision(&self, _revision: &NoteRevision) -> ProfileResult<()> {
        Err(ssh_manager_core::profiles::ProfileError::Backend(
            "simulierter DB-Fehler (Test)".to_string(),
        ))
    }
    async fn list_note_revisions(&self, target: NoteTarget) -> ProfileResult<Vec<NoteRevision>> {
        self.inner.list_note_revisions(target).await
    }
}

/// Zusammenspiel-Design (Aufgabenstellung, Abschnitt 4): reine
/// Prioritäts-Logik, direkt getestet, damit ihre Semantik nicht nur
/// implizit über die (schwerer aufzusetzende) `commands::disconnect`-
/// Verdrahtung geprüft wird.
#[test]
fn test_should_suggest_note_shrink_reflects_update_suggestion_priority() {
    assert!(
        should_suggest_note_shrink(false),
        "kein Update-Vorschlag lief -> der Kürzungs-Vorschlag darf laufen"
    );
    assert!(
        !should_suggest_note_shrink(true),
        "der Update-Vorschlag hat bereits einen Dialog gezeigt -> der Kürzungs-Vorschlag \
         muss für DIESES Verbindungsende ausfallen (nie zwei konkurrierende Notiz-Dialoge)"
    );
}

#[test]
fn test_note_shrink_dialog_appears_for_large_note() {
    let server_id = ServerId::new();
    let large_notes = "n".repeat(LARGE_NOTE_DIALOG_THRESHOLD_CHARS);
    let emitter = TestEmitter::default();

    suggest_note_shrink_on_disconnect(&emitter, server_id, "Test-Server".to_string(), &large_notes);

    let events = emitter.events.lock().unwrap().clone();
    assert_eq!(
        events.len(),
        1,
        "eine große Notiz muss genau einen `note-shrink-suggested`-Vorschlag auslösen"
    );
    assert_eq!(events[0].0, "note-shrink-suggested");
    assert_eq!(events[0].1["serverId"], server_id.0.to_string());
    assert_eq!(events[0].1["serverName"], "Test-Server");
}

/// Spec 0057, §4.2, wörtlich: "Nur bei großer Notiz — bei normalen
/// Notizen kein Dialog (nicht nerven)".
#[test]
fn test_note_shrink_dialog_does_not_appear_for_normal_note() {
    let emitter = TestEmitter::default();

    suggest_note_shrink_on_disconnect(
        &emitter,
        ServerId::new(),
        "Test-Server".to_string(),
        "Kurze, normale Notiz.",
    );

    assert!(
        emitter.events.lock().unwrap().is_empty(),
        "eine normal große Notiz darf keinen Dialog auslösen"
    );
}

/// Spec 0079, §6, T5: genau an der Schwelle — 9 999 Zeichen lösen noch
/// nichts aus, 10 000 Zeichen lösen aus.
#[test]
fn test_note_shrink_dialog_threshold_boundary_in_chars() {
    let emitter = TestEmitter::default();
    let just_below = "n".repeat(LARGE_NOTE_DIALOG_THRESHOLD_CHARS - 1);

    suggest_note_shrink_on_disconnect(
        &emitter,
        ServerId::new(),
        "Test-Server".to_string(),
        &just_below,
    );

    assert!(
        emitter.events.lock().unwrap().is_empty(),
        "9 999 Zeichen liegen unter der Schwelle -> kein Vorschlag"
    );

    let at_threshold = "n".repeat(LARGE_NOTE_DIALOG_THRESHOLD_CHARS);
    suggest_note_shrink_on_disconnect(
        &emitter,
        ServerId::new(),
        "Test-Server".to_string(),
        &at_threshold,
    );

    assert_eq!(
        emitter.events.lock().unwrap().len(),
        1,
        "10 000 Zeichen erreichen die Schwelle -> ein Vorschlag"
    );
}

/// Spec 0079, §6, T6: 6 000 Zeichen "ä" sind 12 000 UTF-8-Byte (über der
/// ALTEN Byte-Schwelle von 8 000), aber nur 6 000 Zeichen (unter der
/// NEUEN Zeichen-Schwelle von 10 000) -> kein Vorschlag. Scheitert mit
/// der alten Byte-Zählung (`note_text.len()`).
#[test]
fn test_note_shrink_dialog_counts_chars_not_bytes() {
    let emitter = TestEmitter::default();
    let multi_byte_notes = "ä".repeat(6_000);
    assert_eq!(
        multi_byte_notes.len(),
        12_000,
        "6 000 × 'ä' sind 12 000 UTF-8-Byte"
    );
    assert_eq!(multi_byte_notes.chars().count(), 6_000);

    suggest_note_shrink_on_disconnect(
        &emitter,
        ServerId::new(),
        "Test-Server".to_string(),
        &multi_byte_notes,
    );

    assert!(
        emitter.events.lock().unwrap().is_empty(),
        "6 000 Zeichen liegen unter der Zeichen-Schwelle, obwohl es 12 000 Byte sind"
    );
}

// Spec 0058, Teil 2 (Etappe-4-Review-Fund): der lokale Pseudo-Server
// hat keine `servers`-Zeile, aus der `profile_store.get_server` je eine
// Notiz lesen könnte — die Auflösung (`local_server::synthetic_server`
// vs. `profile_store.get_server`) passiert deshalb VOR diesem Aufruf,
// in `commands::resolve_server_for_note_shrink` (dort direkt getestet
// — `test_resolve_server_for_note_shrink_uses_synthetic_server_for_
// the_local_pseudo_server`). Diese Funktion selbst kennt "lokal" vs.
// "echt" gar nicht mehr — sie bekommt Name/Notiz bereits aufgelöst und
// behandelt jeden Server identisch. spec-reviewer-Fund (Review dieses
// Schritts): ein Test, der hier zusätzlich die Nil-UUID durchreicht,
// wäre tautologisch (diese Funktion kann "lokal" strukturell gar nicht
// mehr unterscheiden) — der eigentliche Fix wird deshalb bewusst NICHT
// hier, sondern an der `commands.rs`-Verzweigung selbst getestet.

/// Spec 0057, §4.2/§6: der KI-Aufruf hinter "Ja, zusammenfassen" ist
/// session-unabhängig — direkt gegen `&dyn AiProvider`/`&dyn
/// OutputRedactor` getestet, ganz ohne `Session`.
#[tokio::test]
async fn test_summarize_note_for_shrink_returns_redacted_text_on_success() {
    let provider = MockAiProvider::new(vec![
        AiEvent::TextDelta("Gekürzte ".to_string()),
        AiEvent::TextDelta("Notiz mit password=hunter2geheim.".to_string()),
        AiEvent::Done,
    ]);
    let contexts = provider.received_contexts_handle();
    let redactor = DefaultOutputRedactor::new();

    let budget = ai_providers::ProviderBudgetGuard::new();
    let emitter = TestEmitter::default();
    let result = summarize_note_for_shrink(
        &provider,
        &budget,
        &emitter,
        Uuid::new_v4(),
        &redactor,
        "Test-Server",
        "Die ursprüngliche Notiz.",
    )
    .await
    .expect("Erfolgsfall muss Some liefern");

    assert!(
        result.contains("Gekürzte Notiz mit"),
        "der zusammengesetzte Text muss ankommen: {result}"
    );
    assert!(
        !result.contains("hunter2geheim"),
        "die ZURÜCKKOMMENDE Kürzung muss redigiert werden, wie normaler KI-Inhalt: {result}"
    );

    let sent = contexts.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0]
            .history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("Die ursprüngliche Notiz."))),
        "die zu kürzende Notiz muss im gesendeten Kontext stehen"
    );
    assert!(
        sent[0].available_actions.is_empty(),
        "reiner Text-Aufruf, kein Tool-Schema angeboten"
    );
}

/// Spiegelbild der obigen Redaction-Prüfung: ein Secret in der
/// AUSGEHENDEN, gespeicherten Notiz darf den Provider nicht unredigiert
/// erreichen (additive Re-Redaction vor jedem `send()`, Spec 0040,
/// Abschnitt 5 — dieselbe Begründung wie bei `generate_rolling_summary`).
#[tokio::test]
async fn test_summarize_note_for_shrink_redacts_the_outgoing_note_too() {
    let provider = MockAiProvider::new(vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let redactor = DefaultOutputRedactor::new();

    let budget = ai_providers::ProviderBudgetGuard::new();
    let emitter = TestEmitter::default();
    summarize_note_for_shrink(
        &provider,
        &budget,
        &emitter,
        Uuid::new_v4(),
        &redactor,
        "Test-Server",
        "Notiz: password=hunter2geheim",
    )
    .await
    .expect("Erfolgsfall muss Some liefern");

    let sent = contexts.lock().unwrap();
    let sent_text = format!("{:?}", sent[0].history);
    assert!(
        !sent_text.contains("hunter2geheim"),
        "die gesendete Notiz muss redigiert sein: {sent_text}"
    );
}

#[tokio::test]
async fn test_summarize_note_for_shrink_returns_none_on_ai_error() {
    let provider = MockAiProvider::new(vec![AiEvent::Error(AiError::RateLimited)]);
    let redactor = DefaultOutputRedactor::new();

    let budget = ai_providers::ProviderBudgetGuard::new();
    let emitter = TestEmitter::default();
    let result = summarize_note_for_shrink(
        &provider,
        &budget,
        &emitter,
        Uuid::new_v4(),
        &redactor,
        "Test-Server",
        "Eine Notiz.",
    )
    .await;

    assert!(
        result.is_none(),
        "ein KI-Fehler muss None liefern, kein Absturz"
    );
}

/// Ein Stream, der ohne `Done`/`Error` einfach endet, muss wie ein
/// Fehlschlag behandelt werden — nicht stillschweigend als Erfolg
/// gewertet (dieselbe Invariante wie bei `generate_rolling_summary`).
#[tokio::test]
async fn test_summarize_note_for_shrink_returns_none_when_stream_ends_without_done() {
    let provider = MockAiProvider::new(vec![AiEvent::TextDelta("halbe Antwort".to_string())]);
    let redactor = DefaultOutputRedactor::new();

    let budget = ai_providers::ProviderBudgetGuard::new();
    let emitter = TestEmitter::default();
    let result = summarize_note_for_shrink(
        &provider,
        &budget,
        &emitter,
        Uuid::new_v4(),
        &redactor,
        "Test-Server",
        "Eine Notiz.",
    )
    .await;

    assert!(result.is_none());
}

#[tokio::test]
async fn test_summarize_note_for_shrink_returns_none_for_empty_note() {
    let provider = MockAiProvider::new(vec![AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let redactor = DefaultOutputRedactor::new();

    let budget = ai_providers::ProviderBudgetGuard::new();
    let emitter = TestEmitter::default();
    let result = summarize_note_for_shrink(
        &provider,
        &budget,
        &emitter,
        Uuid::new_v4(),
        &redactor,
        "Test-Server",
        "   ",
    )
    .await;

    assert!(result.is_none());
    assert!(
        contexts.lock().unwrap().is_empty(),
        "eine leere Notiz darf gar keinen KI-Aufruf auslösen"
    );
}

/// spec-reviewer-Vorgriff: ohne Obergrenze könnte eine geschwätzige
/// Antwort größer als die Original-Notiz ausfallen und das Kürzungsziel
/// strukturell verfehlen (dieselbe Fehlerklasse wie `compaction::
/// SUMMARY_MAX_BYTES`).
#[tokio::test]
async fn test_summarize_note_for_shrink_caps_the_returned_text() {
    let huge_reply = "x".repeat(NOTE_SHRINK_MAX_BYTES * 3);
    let provider = MockAiProvider::new(vec![AiEvent::TextDelta(huge_reply), AiEvent::Done]);
    let redactor = DefaultOutputRedactor::new();

    let budget = ai_providers::ProviderBudgetGuard::new();
    let emitter = TestEmitter::default();
    let result = summarize_note_for_shrink(
        &provider,
        &budget,
        &emitter,
        Uuid::new_v4(),
        &redactor,
        "Test-Server",
        "Eine Notiz.",
    )
    .await
    .expect("Erfolgsfall muss Some liefern");

    assert!(
        result.len() <= NOTE_SHRINK_MAX_BYTES,
        "die zurückgelieferte Kürzung muss auf NOTE_SHRINK_MAX_BYTES gedeckelt sein: {}",
        result.len()
    );
}

#[tokio::test(start_paused = true)]
async fn test_summarize_note_for_shrink_times_out_instead_of_hanging_forever() {
    let provider = MockAiProvider::new(vec![]); // liefert nie Done/Error
    let redactor = DefaultOutputRedactor::new();

    let budget = ai_providers::ProviderBudgetGuard::new();
    let emitter = TestEmitter::default();
    let call = summarize_note_for_shrink(
        &provider,
        &budget,
        &emitter,
        Uuid::new_v4(),
        &redactor,
        "Test-Server",
        "Eine Notiz.",
    );
    let advancer =
        tokio::time::advance(NOTE_SHRINK_CALL_TIMEOUT + std::time::Duration::from_secs(1));

    let (result, ()) = tokio::join!(call, advancer);
    assert!(
        result.is_none(),
        "Timeout muss zuverlässig als Fehlschlag behandelt werden"
    );
}

/// Spec 0057, §4.2, KRITISCH: "Ja, zusammenfassen" mündet in den
/// Diff-Bestätigungsdialog (hier: `note-update-suggested`, exakt
/// wiederverwendet) — erst NACH Zustimmung wird die gespeicherte Notiz
/// überschrieben.
#[tokio::test]
async fn test_execute_note_shrink_request_emits_diff_and_persists_only_after_approval() {
    let provider = MockAiProvider::new(vec![
        AiEvent::TextDelta("Gekürzte Fassung.".to_string()),
        AiEvent::Done,
    ]);
    let redactor = DefaultOutputRedactor::new();
    let server_id = ServerId::new();
    let profile_store = crate::test_support::InMemoryProfileStore::new().with_server(
        server_with_notes(server_id, "Die lange, ursprüngliche Notiz."),
    );
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();

    let target = crate::orchestration::ProfileStoreNoteShrinkTarget {
        profile_store: &profile_store,
        server_id,
        provider_label: "Test-Provider".to_string(),
        model: "test-model".to_string(),
    };
    let budget = ai_providers::ProviderBudgetGuard::new();
    let flow = execute_note_shrink_request(
        Uuid::new_v4(),
        server_id,
        &provider,
        &budget,
        &redactor,
        &emitter,
        &target,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "note-update-suggested")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                // Kernaussage: bis hierhin (der Diff-Dialog ist bereits
                // angezeigt) darf die gespeicherte Notiz noch NICHT
                // verändert sein.
                assert_eq!(
                    profile_store.get_server(&server_id).await.unwrap().notes,
                    "Die lange, ursprüngliche Notiz.",
                    "vor der Nutzer-Bestätigung darf sich an der gespeicherten Notiz nichts \
                     ändern"
                );
                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Approve)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    tokio::join!(flow, responder);

    let events = emitter.events.lock().unwrap().clone();
    assert_eq!(
        events.len(),
        2,
        "note-update-suggested, dann (nach Zustimmung + erfolgreichem Schreiben) \
         note-shrink-succeeded — s. `emit_note_shrink_succeeded`-Doc-Kommentar"
    );
    assert_eq!(events[0].0, "note-update-suggested");
    assert_eq!(
        events[0].1["previousNoteContent"],
        "Die lange, ursprüngliche Notiz."
    );
    assert_eq!(
        events[0].1["action"]["ProposeNoteUpdate"]["new_content"],
        "Gekürzte Fassung."
    );
    assert_eq!(events[1].0, "note-shrink-succeeded");
    assert_eq!(events[1].1["serverId"], server_id.0.to_string());

    assert_eq!(
        profile_store.get_server(&server_id).await.unwrap().notes,
        "Gekürzte Fassung.",
        "nach der Bestätigung muss die gespeicherte Notiz die gekürzte Fassung tragen"
    );
    let revisions = profile_store.note_revisions.lock().unwrap();
    assert_eq!(revisions.len(), 1);
    assert_eq!(
        revisions[0].edited_by,
        NoteEditor::Ai {
            provider: "Test-Provider".to_string(),
            model: "test-model".to_string(),
        }
    );
}

/// Regressionstest für die zentrale Invariante aus der Aufgabenstellung
/// ("Gespeicherte Notiz wird nie ohne Diff-Bestätigung verändert"): eine
/// Ablehnung darf die gespeicherte Notiz nicht anfassen.
#[tokio::test]
async fn test_execute_note_shrink_request_denied_leaves_note_unchanged() {
    let provider = MockAiProvider::new(vec![
        AiEvent::TextDelta("Gekürzte Fassung.".to_string()),
        AiEvent::Done,
    ]);
    let redactor = DefaultOutputRedactor::new();
    let server_id = ServerId::new();
    let profile_store = crate::test_support::InMemoryProfileStore::new().with_server(
        server_with_notes(server_id, "Die lange, ursprüngliche Notiz."),
    );
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();

    let target = crate::orchestration::ProfileStoreNoteShrinkTarget {
        profile_store: &profile_store,
        server_id,
        provider_label: "Test-Provider".to_string(),
        model: "test-model".to_string(),
    };
    let budget = ai_providers::ProviderBudgetGuard::new();
    let flow = execute_note_shrink_request(
        Uuid::new_v4(),
        server_id,
        &provider,
        &budget,
        &redactor,
        &emitter,
        &target,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "note-update-suggested")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Deny)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    tokio::join!(flow, responder);

    assert_eq!(
        profile_store.get_server(&server_id).await.unwrap().notes,
        "Die lange, ursprüngliche Notiz.",
        "eine Ablehnung darf die gespeicherte Notiz nicht verändern"
    );
    assert!(profile_store.note_revisions.lock().unwrap().is_empty());
}

/// Spec 0057, §4.2/§6: "KI-Aufruf schlägt fehl → Fehlermeldung,
/// gespeicherte Notiz unverändert, kein Hang."
#[tokio::test]
async fn test_execute_note_shrink_request_ai_failure_emits_failed_event_and_leaves_note_unchanged()
{
    let provider = MockAiProvider::new(vec![AiEvent::Error(AiError::RateLimited)]);
    let redactor = DefaultOutputRedactor::new();
    let server_id = ServerId::new();
    let profile_store = crate::test_support::InMemoryProfileStore::new()
        .with_server(server_with_notes(server_id, "Unveränderte Notiz."));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();

    let target = crate::orchestration::ProfileStoreNoteShrinkTarget {
        profile_store: &profile_store,
        server_id,
        provider_label: "Test-Provider".to_string(),
        model: "test-model".to_string(),
    };
    let budget = ai_providers::ProviderBudgetGuard::new();
    execute_note_shrink_request(
        Uuid::new_v4(),
        server_id,
        &provider,
        &budget,
        &redactor,
        &emitter,
        &target,
        &confirmations,
    )
    .await;

    let events = emitter.events.lock().unwrap().clone();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "note-shrink-failed");
    assert_eq!(events[0].1["serverId"], server_id.0.to_string());

    assert_eq!(
        profile_store.get_server(&server_id).await.unwrap().notes,
        "Unveränderte Notiz."
    );
    assert!(profile_store.note_revisions.lock().unwrap().is_empty());
}

/// Kein Hang: bleibt der Diff-Dialog unbeantwortet, muss die
/// Bestätigung nach `PENDING_ACTION_CONFIRM_TIMEOUT` als Ablehnung
/// behandelt werden (Spec 0046, Fund 4 — dieselbe Invariante wie beim
/// regulären `ProposeNoteUpdate`-Ablauf).
#[tokio::test(start_paused = true)]
async fn test_execute_note_shrink_request_unanswered_confirmation_times_out_as_deny() {
    let provider = MockAiProvider::new(vec![
        AiEvent::TextDelta("Gekürzte Fassung.".to_string()),
        AiEvent::Done,
    ]);
    let redactor = DefaultOutputRedactor::new();
    let server_id = ServerId::new();
    let profile_store = crate::test_support::InMemoryProfileStore::new()
        .with_server(server_with_notes(server_id, "Unveränderte Notiz."));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();

    let target = crate::orchestration::ProfileStoreNoteShrinkTarget {
        profile_store: &profile_store,
        server_id,
        provider_label: "Test-Provider".to_string(),
        model: "test-model".to_string(),
    };
    let budget = ai_providers::ProviderBudgetGuard::new();
    let flow = execute_note_shrink_request(
        Uuid::new_v4(),
        server_id,
        &provider,
        &budget,
        &redactor,
        &emitter,
        &target,
        &confirmations,
    );
    let advancer = async {
        loop {
            if !emitter.events.lock().unwrap().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
        tokio::time::advance(PENDING_ACTION_CONFIRM_TIMEOUT + std::time::Duration::from_secs(1))
            .await;
    };

    tokio::join!(flow, advancer);

    assert_eq!(
        profile_store.get_server(&server_id).await.unwrap().notes,
        "Unveränderte Notiz.",
        "ein Timeout muss wie eine Ablehnung behandelt werden — die Notiz bleibt unverändert"
    );
    assert!(profile_store.note_revisions.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_execute_note_shrink_request_emits_failed_event_when_server_not_found() {
    let provider = MockAiProvider::new(vec![AiEvent::Done]);
    let redactor = DefaultOutputRedactor::new();
    let profile_store = crate::test_support::InMemoryProfileStore::new();
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let server_id = ServerId::new();

    let target = crate::orchestration::ProfileStoreNoteShrinkTarget {
        profile_store: &profile_store,
        server_id,
        provider_label: "Test-Provider".to_string(),
        model: "test-model".to_string(),
    };
    let budget = ai_providers::ProviderBudgetGuard::new();
    execute_note_shrink_request(
        Uuid::new_v4(),
        server_id,
        &provider,
        &budget,
        &redactor,
        &emitter,
        &target,
        &confirmations,
    )
    .await;

    let events = emitter.events.lock().unwrap().clone();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "note-shrink-failed");
}

/// spec-reviewer-Fund (Review dieses Schritts, Spec 0039 §3): eine
/// gespeicherte Notiz ist eine der vier untrusted Quellen und MUSS
/// gefenced in den Prompt eingehen, genau wie beim Sende-Pfad
/// (`compaction::compact_for_send`).
#[tokio::test]
async fn test_summarize_note_for_shrink_fences_the_note_as_untrusted_content() {
    let provider = MockAiProvider::new(vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let redactor = DefaultOutputRedactor::new();

    let budget = ai_providers::ProviderBudgetGuard::new();
    let emitter = TestEmitter::default();
    summarize_note_for_shrink(
        &provider,
        &budget,
        &emitter,
        Uuid::new_v4(),
        &redactor,
        "web-01",
        "Eine Notiz mit Inhalt.",
    )
    .await
    .expect("Erfolgsfall muss Some liefern");

    let sent = contexts.lock().unwrap();
    let sent_text = format!("{:?}", sent[0].history);
    assert!(
        sent_text.contains("<server_note>") && sent_text.contains("</server_note>"),
        "die Notiz muss über `fence_untrusted(UntrustedKind::ServerNote, ...)` eingebettet \
         werden: {sent_text}"
    );
    assert!(
        sent_text.contains("web-01"),
        "die Quelle (Servername) muss im Fencing-Tag stehen: {sent_text}"
    );
}

/// spec-reviewer-Fund (Review dieses Schritts): Lost-Update-Schutz — hat
/// sich die gespeicherte Notiz zwischen dem KI-Aufruf (der den
/// `previousNoteContent`-Stand für den Diff liest) und der
/// tatsächlichen Nutzer-Zustimmung geändert, darf die Zusammenfassung
/// NICHT blind darüberschreiben — der Nutzer hat nur einem Diff gegen
/// den ALTEN Stand zugestimmt.
#[tokio::test]
async fn test_execute_note_shrink_request_aborts_when_note_changed_before_approval() {
    let provider = MockAiProvider::new(vec![
        AiEvent::TextDelta("Gekürzte Fassung.".to_string()),
        AiEvent::Done,
    ]);
    let redactor = DefaultOutputRedactor::new();
    let server_id = ServerId::new();
    let profile_store = crate::test_support::InMemoryProfileStore::new()
        .with_server(server_with_notes(server_id, "Ursprüngliche Notiz."));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();

    let target = crate::orchestration::ProfileStoreNoteShrinkTarget {
        profile_store: &profile_store,
        server_id,
        provider_label: "Test-Provider".to_string(),
        model: "test-model".to_string(),
    };
    let budget = ai_providers::ProviderBudgetGuard::new();
    let flow = execute_note_shrink_request(
        Uuid::new_v4(),
        server_id,
        &provider,
        &budget,
        &redactor,
        &emitter,
        &target,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "note-update-suggested")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                // Der Nutzer ändert die Notiz selbst, WÄHREND der
                // Diff-Dialog noch offen ist — z. B. über die normale
                // Notiz-Bearbeitung in einem anderen Fenster.
                let revision = ssh_manager_core::profiles::record_revision(
                    NoteTarget::Server(server_id),
                    "Vom Nutzer inzwischen manuell geänderte Notiz.".to_string(),
                    NoteEditor::User,
                );
                profile_store.record_note_revision(&revision).await.unwrap();

                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Approve)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    tokio::join!(flow, responder);

    assert_eq!(
        profile_store.get_server(&server_id).await.unwrap().notes,
        "Vom Nutzer inzwischen manuell geänderte Notiz.",
        "die zwischenzeitliche manuelle Änderung darf NICHT von der (gegen den alten Stand \
         erzeugten) Zusammenfassung überschrieben werden"
    );
    let events = emitter.events.lock().unwrap().clone();
    assert_eq!(
        events.last().unwrap().0,
        "note-shrink-failed",
        "der Abbruch muss dem Nutzer sichtbar gemeldet werden, nicht still verworfen: \
         {events:?}"
    );
}

/// spec-reviewer-Fund (Review dieses Schritts): ohne dieses Event hätte
/// der Nutzer nach "Annehmen" angenommen, die Notiz sei jetzt gekürzt,
/// obwohl der DB-Schreibvorgang fehlschlug.
#[tokio::test]
async fn test_execute_note_shrink_request_persist_failure_emits_failed_event() {
    let provider = MockAiProvider::new(vec![
        AiEvent::TextDelta("Gekürzte Fassung.".to_string()),
        AiEvent::Done,
    ]);
    let redactor = DefaultOutputRedactor::new();
    let server_id = ServerId::new();
    let profile_store = FailingRecordProfileStore::default()
        .with_server(server_with_notes(server_id, "Ursprüngliche Notiz."));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();

    let target = crate::orchestration::ProfileStoreNoteShrinkTarget {
        profile_store: &profile_store,
        server_id,
        provider_label: "Test-Provider".to_string(),
        model: "test-model".to_string(),
    };
    let budget = ai_providers::ProviderBudgetGuard::new();
    let flow = execute_note_shrink_request(
        Uuid::new_v4(),
        server_id,
        &provider,
        &budget,
        &redactor,
        &emitter,
        &target,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "note-update-suggested")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Approve)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    tokio::join!(flow, responder);

    let events = emitter.events.lock().unwrap().clone();
    assert_eq!(
        events.last().unwrap().0,
        "note-shrink-failed",
        "ein fehlgeschlagener DB-Schreibvorgang nach Zustimmung darf nicht still bleiben: \
         {events:?}"
    );
}

// --- Spec 0012: KI-generierte Dokumente -------------------------------

/// Spec 0012, Abschnitt 2/3: `GenerateDocument` läuft weder durch die
/// Filter-Engine noch durch einen Bestätigungsdialog. `test_session`s
/// Standard-`NoRulesPolicyStore` würde ein `SuggestCommand` auf
/// `Confirm` landen lassen und ohne Responder-Task ewig hängen bleiben
/// — dass dieser Test ohne einen solchen Responder sauber durchläuft,
/// beweist bereits, dass `GenerateDocument` diesen Pfad nie erreicht.
#[tokio::test]
async fn test_generate_document_emits_event_without_filter_engine_or_confirmation() {
    let session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::GenerateDocument {
                title: "Analyse".to_string(),
                content_markdown: "# Analyse\n\nInhalt.".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let events = emitter.events.lock().unwrap().clone();
    let event_names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(event_names, vec!["chat-document-generated"]);

    let (_, payload) = &events[0];
    assert_eq!(payload["title"], "Analyse");
    assert_eq!(payload["contentMarkdown"], "# Analyse\n\nInhalt.");

    // Spec 0012, Abschnitt 5: landet als Assistant-Text in der Historie.
    let history = session.context.lock().await.history.clone();
    assert!(history.iter().any(|m| matches!(
        &m.content,
        MessageContent::Text(t) if t.contains("Inhalt.")
    )));
}

// --- Spec 0011: Regel-Schnellvorschlag im Bestätigungsdialog ---------

/// Spec 0011, Abschnitt 3: "legt die Regel an ... löst danach die
/// wartende Confirm-Entscheidung ... auf, exakt wie ein
/// `respond_to_action`-Aufruf mit `Approve`". Der eigentliche
/// `accept_and_create_rule`-Tauri-Command (`app_shell::commands`) ist ein
/// dünner Wrapper genau um diese zwei Aufrufe
/// (`crate::rule_suggestions::create_quick_rule` +
/// `ConfirmationRegistry::resolve(..., Approve)`) — dieser Test bildet
/// exakt diese Kombination nach und prüft beide Effekte: die Regel
/// landet in einer echten `SqlitePolicyStore`, **und** das ursprünglich
/// vorgeschlagene Kommando wird tatsächlich über `MockSshTransport`
/// ausgeführt (nicht nur "irgendwie aufgelöst").
#[tokio::test]
async fn test_accept_and_create_rule_creates_rule_and_resolves_confirm_like_approve() {
    let session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "systemctl status nginx".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("systemctl status nginx", output("active")),
    );
    // `test_session`s Standard `NoRulesPolicyStore` landet für jedes
    // Kommando auf `Confirm` (kein Allow-Match) — genau der Pfad, den
    // dieser Test braucht.
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let dir = tempfile::tempdir().expect("Temp-Verzeichnis sollte anlegbar sein");
    let policy_store = persistence_sqlite::SqliteProfileStore::connect(&dir.path().join("test.db"))
        .await
        .expect("frische SQLite-Datenbank mit angewendeten Migrationen sollte immer aufbaubar sein")
        .policy_store();

    let turn = run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "chat-action-proposed")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                // Nachbau von `commands::accept_and_create_rule`:
                // zuerst die Regel anlegen, dann exakt wie `Approve`
                // auflösen.
                crate::rule_suggestions::create_quick_rule(
                    &policy_store,
                    crate::dto::PatternType::Glob,
                    "systemctl status *".to_string(),
                    ssh_manager_core::filter::Scope::Global,
                    None,
                )
                .await
                .unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Approve)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    tokio::join!(turn, responder);

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec!["chat-action-proposed", "chat-action-result"],
        "die Regel-Erstellung muss die Ausführung des ursprünglichen \
         Kommandos wie ein normales Approve auslösen"
    );

    let rules = policy_store.list_all().await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0].pattern,
        ssh_manager_core::filter::Pattern::Glob("systemctl status *".to_string())
    );
    assert_eq!(rules[0].action, ssh_manager_core::filter::RuleAction::Allow);
    assert_eq!(rules[0].scope, ssh_manager_core::filter::Scope::Global);
    assert_eq!(
        rules[0].priority, 0,
        "keine Priorität angegeben -> Default 0"
    );
}

/// Regressionstest für den unabhängigen Review-Pass (Spec 0007/0008/
/// 0011): akzeptiert der Nutzer eine Schnellregel für ein im
/// Bestätigungsdialog BEARBEITETES Kommando, muss tatsächlich das
/// bearbeitete Kommando ausgeführt werden — nicht das ursprüngliche,
/// unbearbeitete. Vorher löste `commands::accept_and_create_rule` immer
/// mit `ActionUserDecision::Approve` auf, was IMMER die ursprüngliche
/// `AiAction` ausführt, unabhängig davon, was der Nutzer im
/// Bearbeiten-Feld sah/anpasste. Bildet `commands::accept_and_create_rule`
/// mit gesetztem `edited_command` nach (Auflösung über
/// `EditThenApprove`, wie beim regulären "Ausführen"-Button) und beweist
/// über einen `MockSshTransport`, der NUR auf das bearbeitete Kommando
/// antwortet, dass tatsächlich dieses ausgeführt wird — würde
/// stattdessen (der Bug) das ursprüngliche Kommando ausgeführt, schlägt
/// es am unkonfigurierten `MockSshTransport`-Eintrag fehl statt einen
/// `chat-action-result` zu liefern.
#[tokio::test]
async fn test_accept_and_create_rule_with_edited_command_executes_the_edited_command() {
    let session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "rm -rf /var/log/*".to_string(),
            }),
            AiEvent::Done,
        ],
        // Bewusst NUR auf das bearbeitete Kommando konfiguriert — liefe
        // stattdessen das ursprüngliche `rm -rf /var/log/*`, schlägt
        // der Mock mit einem Fehler statt einer Antwort fehl.
        MockSshTransport::default().with_response("ls /var/log", output("access.log")),
    );
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let dir = tempfile::tempdir().expect("Temp-Verzeichnis sollte anlegbar sein");
    let policy_store = persistence_sqlite::SqliteProfileStore::connect(&dir.path().join("test.db"))
        .await
        .expect("frische SQLite-Datenbank mit angewendeten Migrationen sollte immer aufbaubar sein")
        .policy_store();

    let turn = run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "chat-action-proposed")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                // Nachbau von `commands::accept_and_create_rule` MIT
                // gesetztem `edited_command`.
                crate::rule_suggestions::create_quick_rule(
                    &policy_store,
                    crate::dto::PatternType::Glob,
                    "ls /var/log".to_string(),
                    ssh_manager_core::filter::Scope::Global,
                    None,
                )
                .await
                .unwrap();
                confirmations
                    .resolve(
                        &action_id,
                        ActionUserDecision::EditThenApprove {
                            command: "ls /var/log".to_string(),
                        },
                    )
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    tokio::join!(turn, responder);

    let events = emitter.events.lock().unwrap().clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec!["chat-action-proposed", "chat-action-result"],
        "das bearbeitete Kommando muss erfolgreich ausgeführt werden, nicht das \
         ursprüngliche (das am unkonfigurierten Mock-Eintrag fehlschlagen würde)"
    );
    let (_, result_payload) = events
        .iter()
        .find(|(name, _)| name == "chat-action-result")
        .expect("chat-action-result sollte vorhanden sein");
    let result_text = serde_json::to_string(result_payload).unwrap();
    assert!(
        result_text.contains("access.log"),
        "Ausgabe des bearbeiteten Kommandos sollte im Ergebnis stehen, war: {result_text}"
    );
}

// --- Spec 0016, Abschnitt 6: Ziel-Auflösung & Fehler-Containment -------

/// Spec 0016, Abschnitt 6, letzter Absatz — Regressionstest für den
/// gemeldeten Bug: ein fehlerhafter Tool-Call darf **ausschließlich**
/// als Chat-Fehlermeldung erscheinen, nie die Session/Verbindung
/// beenden. Simuliert über einen `MockAiProvider`, der direkt
/// `AiEvent::Error` liefert — exakt das Ereignis, das `ai-providers`
/// bei einem Tool-Call-Parse-/Validierungsfehler produziert (s.
/// `ai_providers::anthropic::finalize_tool_use`/
/// `ai_providers::openai_compatible::finalize_tool_call`, beide geben
/// bei Fehlern `AiEvent::Error` zurück statt zu paniken). Der Beweis,
/// dass die Session danach weiter nutzbar bleibt: ein zweites,
/// unabhängiges Kommando läuft direkt im Anschluss über dieselbe
/// `Session` erfolgreich durch.
#[tokio::test]
async fn test_malformed_tool_call_yields_chat_error_without_ending_session() {
    let session = test_session(
        vec![
            AiEvent::Error(AiError::InvalidResponse(
                "target_id ist keine gültige UUID: invalid character".to_string(),
            )),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("echo still-alive", output("still-alive")),
    );
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let events = emitter.events.lock().unwrap().clone();
    let event_names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        event_names,
        vec!["chat-error"],
        "ein fehlerhafter Tool-Call darf nur als Chat-Fehlermeldung erscheinen"
    );
    // Spec 0024, Abschnitt 5: `AiError::code()` muss mitgegeben werden,
    // damit das Frontend übersetzen kann (unabhängiger Review-Pass /
    // Spec-Audit-Fund — zuvor kam nur der rohe deutsche Text an).
    assert_eq!(events[0].1["code"].as_str(), Some("AI_INVALID_RESPONSE"),);

    let result = session
        .transport
        .lock()
        .await
        .execute("echo still-alive")
        .await;
    assert!(
        result.is_ok(),
        "die Session/Verbindung darf durch den fehlerhaften Tool-Call nicht beendet \
         werden — sie muss danach unverändert nutzbar bleiben"
    );
}

/// Spec 0016, Abschnitt 6: `target: "current_server"` löst auf
/// `session.server_id` auf — die KI nennt nie eine ID.
#[tokio::test]
async fn test_propose_note_update_current_server_resolves_to_session_server_id() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::ProposeNoteUpdate {
                target: NoteTargetSelector::CurrentServer,
                new_content: "Notiz".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    let expected_server_id = session.server_id;
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let turn = run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = async {
        loop {
            let action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "chat-action-proposed")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Approve)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::join!(turn, responder);

    let revisions = profile_store.note_revisions.lock().unwrap().clone();
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].target, NoteTarget::Server(expected_server_id));
}

// --- Spec 0040, Abschnitt 6: "In Notiz übernehmen" -----------------

/// `propose_note_from_chat_content` muss den bestehenden Notizinhalt
/// des aktuellen Servers laden und den übernommenen Chat-Inhalt daran
/// anhängen (nicht ersetzen) — derselbe `ProposeNoteUpdate`-Dialog wie
/// bei einem KI-Vorschlag zeigt diesen vollständigen neuen Text als
/// Diff-Vorschau an.
#[tokio::test]
async fn test_propose_note_from_chat_content_appends_to_existing_note_and_persists_on_approve() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    let server_id = session.server_id;
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let now = chrono::Utc::now();
    profile_store.servers.lock().unwrap().insert(
        server_id,
        Server {
            id: server_id,
            name: "Test-Server".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: ssh_manager_core::profiles::AuthMethod::Agent,
            notes: "Bestehende Notiz".to_string(),
            jump_host: None,
            post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            created_at: now,
            updated_at: now,
        },
    );
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let action_future = propose_note_from_chat_content(
        &session,
        session_id,
        "Aus dem Chat übernommener Inhalt".to_string(),
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = approve_first_proposed_action(&emitter, &confirmations);
    let ((), ()) = tokio::join!(
        async {
            action_future.await;
        },
        responder
    );

    let events = emitter.events.lock().unwrap().clone();
    let (_, proposed_payload) = events
        .iter()
        .find(|(name, _)| name == "chat-action-proposed")
        .expect("chat-action-proposed muss gesendet worden sein");
    assert_eq!(
        proposed_payload["action"]["ProposeNoteUpdate"]["new_content"],
        serde_json::json!("Bestehende Notiz\n\nAus dem Chat übernommener Inhalt"),
        "die Vorschau muss die bestehende Notiz plus den übernommenen Inhalt zeigen: \
         {proposed_payload}"
    );

    let revisions = profile_store.note_revisions.lock().unwrap().clone();
    assert_eq!(
        revisions.len(),
        1,
        "Bestätigung muss die Notiz tatsächlich aktualisieren"
    );
    assert_eq!(revisions[0].target, NoteTarget::Server(server_id));
    assert_eq!(
        revisions[0].content,
        "Bestehende Notiz\n\nAus dem Chat übernommener Inhalt"
    );
}

/// Ohne bestehenden Notizinhalt (leerer String) wird der übernommene
/// Inhalt nicht mit einem führenden Leerzeilen-Präfix versehen — reiner
/// Ersatz statt einer sichtbar leeren Anhängung.
#[tokio::test]
async fn test_propose_note_from_chat_content_with_empty_existing_note_uses_content_directly() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    let server_id = session.server_id;
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let now = chrono::Utc::now();
    profile_store.servers.lock().unwrap().insert(
        server_id,
        Server {
            id: server_id,
            name: "Test-Server".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: ssh_manager_core::profiles::AuthMethod::Agent,
            notes: String::new(),
            jump_host: None,
            post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            created_at: now,
            updated_at: now,
        },
    );
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let action_future = propose_note_from_chat_content(
        &session,
        session_id,
        "Erster Inhalt".to_string(),
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = approve_first_proposed_action(&emitter, &confirmations);
    let ((), ()) = tokio::join!(
        async {
            action_future.await;
        },
        responder
    );

    let events = emitter.events.lock().unwrap().clone();
    let (_, proposed_payload) = events
        .iter()
        .find(|(name, _)| name == "chat-action-proposed")
        .expect("chat-action-proposed muss gesendet worden sein");
    assert_eq!(
        proposed_payload["action"]["ProposeNoteUpdate"]["new_content"],
        serde_json::json!("Erster Inhalt"),
    );
}

// --- Spec 0096: Geheimnisse in der Datenbank -------------------------

/// Spec 0096, Abschnitt 7: hält die Form fest, auf der alle Tests dieser
/// Spec aufbauen — `password=Geheim-0096` wird vom Session-Redactor
/// vollständig durch den Platzhalter ersetzt, es bleibt kein Rest des
/// Schlüsselworts stehen. Bricht diese Annahme (z. B. weil eine Regel
/// künftig nur den Wert ersetzt), müssen die Erwartungen in T2/T10
/// („nur Platzhalter → kein Titel/kein Vorschlag") nachgezogen werden.
#[test]
fn test_spec_0096_secret_form_is_fully_replaced_by_the_placeholder() {
    let redacted = DefaultOutputRedactor::new().redact_text("password=Geheim-0096");
    assert_eq!(redacted, "[REDACTED]");
}
