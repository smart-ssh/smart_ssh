//! Session assembly after a successful transport (issue #839).
//!
//! Everything that turns a connected transport into a [`Session`] without
//! needing the Tauri runtime: the OS and root probes, the sudo password, the
//! redactor (with the extension's known secrets), the system prompt, the
//! chat history and the `SessionParts` literal. What needs Tauri (the
//! extension scope of the local pseudo-server, the prompt language, the
//! second-opinion settings) is resolved by the caller and handed in as plain
//! values in [`SessionBuildInput`].
//!
//! The redactor is attached to the session at construction time, before the
//! session is returned (and so before it can be registered): no output of
//! the session can reach the AI context or the structured log unredacted.

use std::sync::Arc;

use ai_providers::ProviderBudgetGuard;
use ssh_manager_core::ai::{fence_untrusted, UntrustedKind};
use ssh_manager_core::ai::{AiProvider, ChatMessage, MessageContent, Role, SessionContext};
use ssh_manager_core::filter::{EffectiveScope, EvalContext, FilterEngine, PolicyStore};
use ssh_manager_core::profiles::{effective_notes_sections, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{ConnectionLossWatch, SshTransport};

use persistence_sqlite::AiProviderConfig;

use crate::ai_usage::{AiUsageReporting, AiUsageSource};
use crate::error::CommandError;
use crate::session::{
    history_contains_untrusted_content, AiConfig, ChatPersistence, SafetyState, Session,
    SessionParts, TransportState,
};
use crate::session_extension::ScopeLookup;
use crate::state::{AppState, SessionId};
use crate::system_prompt::PromptLanguage;

/// A side-call provider (second opinion, injection check) as resolved by the
/// caller, with its budget guard and usage source.
pub type ResolvedSideProvider =
    Option<(Box<dyn AiProvider>, Arc<ProviderBudgetGuard>, AiUsageSource)>;

/// Everything [`build_session`] needs that is not in [`AppState`] and not
/// Tauri-free to obtain.
pub struct SessionBuildInput {
    pub server_id: ServerId,
    pub session_id: SessionId,
    /// The stored profile, or the synthetic one for the local pseudo-server.
    pub server: Server,
    pub resume: Option<uuid::Uuid>,
    pub persist_chat_session: bool,
    pub active_config: AiProviderConfig,
    pub ai_provider: Box<dyn AiProvider>,
    pub ai_provider_budget: Arc<ProviderBudgetGuard>,
    /// Extension scope of the server (issue #843).
    pub scope_lookup: ScopeLookup,
    pub prompt_language: PromptLanguage,
    pub second_opinion: ResolvedSideProvider,
    /// Second opinion is on but could not be set up (issue #102).
    pub second_opinion_setup_failed: bool,
    /// Resolved provider for the injection check; `None` when the check is
    /// off for the server or no provider is available.
    pub injection_check: ResolvedSideProvider,
    pub red_risk_always_confirm: bool,
}

/// The assembled session and what the caller needs to register it.
pub struct BuiltSession {
    pub session: Session,
    pub chat_session_id: Option<uuid::Uuid>,
    pub loss_watch: Option<ConnectionLossWatch>,
}

/// Spec 0034, Abschnitt 2: nur echte Server bekommen eine `chat_sessions`-
/// Zeile (der lokale Pseudo-Server hat keine `servers`-Zeile), und nur wenn
/// der Aufrufer Persistenz verlangt (MCP nicht, Spec 0040 Abschnitt 4).
pub fn should_create_chat_session(is_local: bool, persist_chat_session: bool) -> bool {
    !is_local && persist_chat_session
}

/// Spec 0064: the `uname -a` banner as its own fenced history message.
pub fn build_os_banner_message(sanitized_os: &str) -> ChatMessage {
    ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::Text(fence_untrusted(
            UntrustedKind::RemoteOsInfo,
            "uname -a",
            sanitized_os,
        )),
    }
}

/// Raw parts of the system prompt (Spec 0057, §4.1) and whether notes are
/// present (Spec 0039, section 5). `local_notes` are the notes of the
/// synthetic local server and are used instead of a store lookup when
/// `server_id` is the local pseudo-server (it has no `servers` row).
pub async fn build_session_system_context(
    language: PromptLanguage,
    server_name: &str,
    server_id: &ServerId,
    tags: &[String],
    local_notes: &str,
    profile_store: &dyn ProfileStore,
    policy_store: &persistence_sqlite::SqlitePolicyStore,
) -> (crate::compaction::SystemContextParts, bool) {
    // Spec 0032: the local pseudo-server has no `servers` row, so
    // `get_server` would always fail for it and silently drop its notes
    // (independent review, docs/adr/0026). A failed lookup is logged: it
    // means security-relevant context vanishes from the prompt. Spec 0039,
    // section 3: sections stay separate so each is fenced on its own.
    let note_sections: Vec<(String, String)> = if crate::dto::is_local(*server_id) {
        if local_notes.trim().is_empty() {
            Vec::new()
        } else {
            vec![(format!("Server \"{server_name}\""), local_notes.to_string())]
        }
    } else {
        match profile_store.get_server(server_id).await {
            Ok(s) => match effective_notes_sections(&s, profile_store).await {
                Ok(sections) => sections,
                Err(err) => {
                    tracing::warn!(
                        server_id = %server_id.0,
                        error = %err,
                        "effective_notes_sections fehlgeschlagen — Session-Kontext enthält keine Notizen",
                    );
                    Vec::new()
                }
            },
            Err(err) => {
                tracing::warn!(
                    server_id = %server_id.0,
                    error = %err,
                    "get_server fehlgeschlagen — Session-Kontext enthält keine Notizen",
                );
                Vec::new()
            }
        }
    };

    let mut context = crate::system_prompt::base_prompt(language, server_name);

    let eval_ctx = EvalContext {
        server_id: *server_id,
        tags: tags.to_vec(),
    };
    let scope = EffectiveScope::from(&eval_ctx);
    let rules = policy_store.rules_for(&scope).await.into_trusted_rules();
    let allow_rules = crate::system_prompt::allow_rule_lines(&rules);

    context.push_str(&crate::system_prompt::allow_rules_section(
        language,
        &allow_rules,
    ));

    let parts = crate::compaction::SystemContextParts {
        base: context,
        note_sections,
        language,
    };
    let has_notes = parts.has_notes();
    (parts, has_notes)
}

/// Builds the session on a connected transport. On an error after the
/// transport was handed in, the transport is disconnected first.
pub async fn build_session(
    state: &AppState,
    mut transport: Box<dyn SshTransport>,
    input: SessionBuildInput,
) -> Result<BuiltSession, CommandError> {
    let SessionBuildInput {
        server_id,
        session_id,
        server,
        resume,
        persist_chat_session,
        active_config,
        ai_provider,
        ai_provider_budget,
        scope_lookup,
        prompt_language,
        second_opinion,
        second_opinion_setup_failed,
        injection_check,
        red_risk_always_confirm,
    } = input;
    let is_local = crate::dto::is_local(server_id);

    let sanitized_os = if let Ok(uname_output) = transport.execute("uname -a").await {
        let uname_text = String::from_utf8_lossy(&uname_output.stdout);
        crate::orchestration::sanitize_uname_output(&uname_text)
    } else {
        None
    };

    // Issue #544: root probe for the file browser's elevation controls. Real
    // SSH sessions only; any failure means "unknown" (never fails connect).
    let is_root = if is_local {
        None
    } else {
        match transport.execute("id -u").await {
            Ok(out) => {
                crate::orchestration::parse_id_u_output(&String::from_utf8_lossy(&out.stdout))
            }
            Err(err) => {
                tracing::debug!(error = %err, "id -u probe failed");
                None
            }
        }
    };

    // Spec 0018, Abschnitt 6: read once; a missing entry is `None`, no hard
    // error. Issue #36: the shared read path logs a keychain read failure.
    // Without the redactor knowing the sudo password it would reach the AI
    // context and the log when `sudo` passes the stdin line on to the
    // executed program (NOPASSWD, valid timestamp).
    let sudo = crate::server_redaction::read_sudo_password_checked_async(
        state.credential_store.clone(),
        server_id,
    )
    .await;
    let sudo_password = sudo.0.clone();

    // Spec 0038, section 2a: known secrets of a registered session extension
    // are masked in addition to the sudo password.
    let extension_scope = scope_lookup.scope.clone();
    let extension_secrets = crate::session_extension::known_secrets(
        state.session_extension.as_deref(),
        extension_scope.as_ref(),
    );
    let (redactor, redaction_incomplete) =
        crate::session_extension::redactor_for_connect(&sudo, &scope_lookup, &extension_secrets);

    let (mut system_context_parts, notes_present) = build_session_system_context(
        prompt_language,
        &server.name,
        &server_id,
        &server.tags,
        &server.notes,
        state.profile_store.as_ref(),
        &state.policy_store,
    )
    .await;
    let extension_present = crate::session_extension::append_extension_section(
        &mut system_context_parts,
        state.session_extension.as_deref(),
        extension_scope.as_ref(),
        redactor.as_ref(),
    )
    .await;
    let notes_present = notes_present || extension_present;
    let system_context = system_context_parts.assemble();
    // Issue #323: the `suggest_command` description follows the prompt language.
    let available_actions =
        crate::system_prompt::session_action_schemas(system_context_parts.language);

    // Spec 0064: the banner is its own fenced history message, built fresh on
    // every connect and put before the (resumed or fresh) history; it is not
    // persisted directly.
    let os_banner_message: Option<ChatMessage> =
        sanitized_os.as_deref().map(build_os_banner_message);

    // Spec 0057, §3.1: resolved once per connect.
    let model_context_window_tokens = crate::compaction::model_context_window_tokens(
        active_config.provider_type,
        &active_config.model,
    );

    let (risk_second_opinion_provider, risk_second_opinion_budget, second_opinion_source) =
        match second_opinion {
            Some((provider, budget, source)) => (Some(provider), Some(budget), Some(source)),
            None => (None, None, None),
        };

    let post_ingest_policy = server.post_ingest_policy;
    // Spec 0102: start directory of terminal and file browser.
    let start_directory = server.start_directory.clone();

    // Spec 0039, section 5.2 / issue #231: the check is `Some` only when the
    // server has it on AND a provider is available; "on without provider"
    // becomes a session notice.
    let (
        injection_check_provider,
        injection_check_budget,
        injection_check_inactive,
        injection_check_source,
    ) = if server.ai_injection_check_enabled {
        match injection_check {
            Some((provider, budget, source)) => (Some(provider), Some(budget), false, Some(source)),
            None => (None, None, true, None),
        }
    } else {
        (None, None, false, None)
    };
    // Issue #610: the side calls share one configured provider.
    let side_call_source = second_opinion_source.or(injection_check_source);
    let ai_usage_reporting = AiUsageReporting {
        observers: state.ai_usage_observers.clone(),
        runtime_session_id: session_id.to_string(),
        main: AiUsageSource {
            provider_config_id: active_config.id.0.to_string(),
            provider_type: active_config.provider_type,
            configured_model: active_config.model.clone(),
        },
        side_call: side_call_source,
    };

    // Spec 0034, Abschnitt 2: the local pseudo-server has no `servers` row, so
    // it gets no `chat_sessions` row (foreign key). The special case belongs
    // here (session construction), not in the filter/risk loop.
    //
    // `resume`: the stored history MUST be loadable (Spec 0034, section 5,
    // point 1); a load failure is a hard error, not a fallback to an empty
    // history. The SSH connection is already up, so both error branches
    // disconnect explicitly (independent review, Spec 0040, section 7).
    let (mut initial_history, chat_session_id, initial_summary) = if let Some(existing_id) = resume
    {
        let store = &state.chat_session_store;
        let loaded = match store.load_session(existing_id).await {
            Ok(loaded) => loaded,
            Err(err) => {
                transport.disconnect().await.ok();
                return Err(err.into());
            }
        };
        if let Err(err) = store.mark_resumed(existing_id).await {
            transport.disconnect().await.ok();
            return Err(err.into());
        }
        // Spec 0057, §2.3: reload the rolling summary, best-effort: without
        // it the next compaction starts a fresh summary.
        let initial_summary = match store.load_summary(existing_id).await {
            Ok(Some((text, rounds_covered))) => Some(crate::compaction::RollingSummary {
                text,
                // Clamped to the loaded rounds: a stored `rounds_covered`
                // can exceed them (unpersisted MCP actions, Spec 0034 §10,
                // or a failed best-effort `append_message`).
                rounds_covered: (rounds_covered.max(0) as usize)
                    .min(crate::compaction::round_count(&loaded)),
            }),
            Ok(None) => None,
            Err(err) => {
                tracing::warn!(error = %err, "session summary could not be loaded on resume");
                None
            }
        };
        // Deliberately NO round truncation here (spec-reviewer finding): it
        // would cut every resumed session to the last rounds regardless of
        // the token budget, and `history_contains_untrusted_content` below
        // could miss earlier injected content. `compact_for_send` runs
        // before the first `send()` anyway.
        (loaded, Some(existing_id), initial_summary)
    } else if !should_create_chat_session(is_local, persist_chat_session) {
        (Vec::new(), None, None)
    } else {
        match state
            .chat_session_store
            .create_session(&server_id, Some(active_config.id.0))
            .await
        {
            Ok(id) => (Vec::new(), Some(id), None),
            Err(err) => {
                // Persistence is not a hard requirement for connecting: this
                // session's history just stays unpersisted.
                tracing::warn!(error = %err, "chat session creation failed");
                (Vec::new(), None, None)
            }
        }
    };
    // Spec 0064: in front of the history, not via `push_history_scoped`
    // (that would persist it again on every resume).
    if let Some(banner) = os_banner_message {
        initial_history.insert(0, banner);
    }

    // Spec 0039, section 5: notes in the prompt, or a resumed history with
    // fenced content, start the session with the untrusted-content flag.
    let starts_with_untrusted_content =
        notes_present || history_contains_untrusted_content(&initial_history);
    // Persisted history never holds MCP-originated messages (Spec 0034 §10),
    // so the flags are `false` throughout (addendum to Spec 0057 §2.1).
    let initial_mcp_origin_flags = vec![false; initial_history.len()];

    let loss_watch = transport.connection_loss_watch();
    let session = Session::new(SessionParts {
        transport: crate::session::SessionTransport::new(transport),
        server_id,
        tags: server.tags,
        redactor: crate::session::SwappableRedactor::new(redactor),
        ai: AiConfig {
            provider: ai_provider,
            provider_budget: ai_provider_budget,
            context: tokio::sync::Mutex::new(SessionContext {
                system_context,
                history: initial_history,
                available_actions,
                max_tokens_hint: None,
            }),
            label: active_config.display_name,
            model: active_config.model,
            system_context_parts: tokio::sync::Mutex::new(system_context_parts),
            model_context_window_tokens,
            summary: tokio::sync::Mutex::new(initial_summary),
            context_usage: Default::default(),
            request_paced_at: tokio::sync::Mutex::new(None),
        },
        safety: SafetyState {
            filter_engine: Box::new(FilterEngine::new(state.policy_store.clone())),
            mcp_origin_flags: std::sync::Mutex::new(initial_mcp_origin_flags),
            sudo_password: crate::session::SwappableSudoPassword::new(sudo_password),
            mcp_confirmation_claim: std::sync::atomic::AtomicBool::new(false),
            risk_second_opinion_provider,
            risk_second_opinion_budget,
            red_risk_always_confirm,
            outgoing_transform: state.outgoing_transform.clone(),
            untrusted_content_ingested: std::sync::atomic::AtomicBool::new(
                starts_with_untrusted_content,
            ),
            post_ingest_policy,
            injection_check_provider,
            injection_check_budget,
            injection_suspected: std::sync::atomic::AtomicBool::new(false),
            injection_suspected_reason: std::sync::Mutex::new(None),
            injection_check_unavailable: std::sync::atomic::AtomicBool::new(false),
            second_opinion_setup_notice_pending: std::sync::atomic::AtomicBool::new(
                second_opinion_setup_failed,
            ),
            injection_check_inactive_notice_pending: std::sync::atomic::AtomicBool::new(
                injection_check_inactive,
            ),
            redaction_incomplete_notice_pending: std::sync::atomic::AtomicBool::new(
                redaction_incomplete,
            ),
        },
        persistence: ChatPersistence {
            chat_content: state.chat_content.clone(),
            chat_session_store: chat_session_id.map(|_| state.chat_session_store.clone()),
            // Spec 0057, §1: same gating as `chat_session_store` (FK, migration 0011).
            ledger_store: chat_session_id.map(|_| state.ledger_store.clone()),
            chat_session_id: tokio::sync::Mutex::new(chat_session_id),
        },
        conn: TransportState {
            terminal: std::sync::Mutex::new(None),
            status: std::sync::Mutex::new(crate::events::ConnectionStatus::Connected),
            pending_action: std::sync::Mutex::new(None),
            auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
            auto_continue_stop_notify: tokio::sync::Notify::new(),
            chat_turn: std::sync::Mutex::new(crate::session::ChatTurnState::default()),
            running_command_cancellations: state.running_command_cancellations.clone(),
            auto_reconnect: Default::default(),
        },
    })
    .with_start_directory(start_directory)
    .with_is_root(is_root)
    .with_usage_reporting(Some(ai_usage_reporting));
    // Byte-wise copy: stays masked if a later lookup fails (issue #843).
    session.redactor.add_secret_values(&extension_secrets);
    Ok(BuiltSession {
        session,
        chat_session_id,
        loss_watch,
    })
}

/// Reconnect (Spec 0105): the sudo password is read again so the redactor
/// knows the current value. Returns the new redactor and the sudo password.
pub async fn redaction_for_reconnect(
    state: &AppState,
    session: &Session,
    server_id: ServerId,
    scope_lookup: &ScopeLookup,
) -> (
    Box<dyn ssh_manager_core::ai::OutputRedactor>,
    Option<secrecy::SecretString>,
) {
    let sudo = crate::server_redaction::read_sudo_password_checked_async(
        state.credential_store.clone(),
        server_id,
    )
    .await;
    let extension_secrets = crate::session_extension::known_secrets(
        state.session_extension.as_deref(),
        scope_lookup.scope.as_ref(),
    );
    let redactor = crate::session_extension::redactor_for_reconnect(
        session,
        &sudo,
        scope_lookup,
        &extension_secrets,
    );
    (redactor, sudo.0)
}

#[cfg(test)]
mod tests;
