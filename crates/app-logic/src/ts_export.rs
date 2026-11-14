//! Issue #919: generates `src/generated/ipc.ts` in the frontend from the Rust
//! IPC types (ts-rs, behind the `ts-export` feature, so a normal build never
//! compiles it). The test compares the generated text with the checked-in file
//! and fails when they differ; `UPDATE_IPC_TS=1` rewrites the file instead
//! (`npm run gen:ipc`).

use std::path::PathBuf;

use ts_rs::TS;

const HEADER: &str = "// generated – do not edit.\n\
// Regenerate with `npm run gen:ipc` (apps/smart-ssh-community/frontend).\n\
// Source: the Rust IPC types in `app-logic` and `ssh_manager_core`.\n\n";

macro_rules! decls {
    ($($t:ty),* $(,)?) => {{
        let cfg = ts_rs::Config::new().with_large_int("number");
        let mut v: Vec<String> = vec![$(format!("export {}", <$t as TS>::decl(&cfg))),*];
        v.sort();
        v
    }};
}

fn generate() -> String {
    use crate::activities::{
        Activity, ActivityOutcome, ActivitySpec, ActivityState, ActivityUpdate,
        InterruptedActivity, QuitRequestedPayload, ResumeSpec,
    };
    use crate::auth_prompt_events::*;
    use crate::auto_reconnect::{AutoReconnectChangedPayload, AutoReconnectPhase};
    use crate::confirmation_settled::{
        ActionConfirmationSettledPayload, ConfirmationSettledReason,
    };
    use crate::dto::*;
    use crate::elevation_events::ElevationAutoDisabledPayload;
    use crate::events::connect_warnings::ConnectWarningsPayload;
    use crate::events::*;
    use crate::status::ConnectionStatus;
    use crate::version::BuildType;
    use ssh_manager_core::ai::{ExtensionMedia, ProviderId, ProviderType};
    use ssh_manager_core::data_change::DataKind;
    use ssh_manager_core::entitlements::{Entitlements, Feature, FeatureLocked, TierId};
    use ssh_manager_core::filter::{Decision, RuleAction, RuleId, Scope, TraceKind};
    use ssh_manager_core::profiles::{AiAction, AskUserOption, NoteTargetSelector};
    use ssh_manager_core::profiles::{GroupId, PostIngestPolicy};
    use ssh_manager_core::risk::{RiskAssessment, RiskLevel};
    use ssh_manager_core::shared::ServerId;
    use ssh_manager_core::ssh::connect_log::StepDetail;
    use ssh_manager_core::ssh::connect_log::{
        AuthMethodKind as ConnectAuthMethodKind, ConnectStep, ConnectStepRecord,
        HostKeyCheckResult, StepStatus,
    };
    use ssh_manager_core::ssh::{ConnectStepSnapshot, ConnectWarning};
    let decls = decls![
        Activity,
        ActivityOutcome,
        ActivitySpec,
        ActivityState,
        ActivityUpdate,
        InterruptedActivity,
        QuitRequestedPayload,
        ResumeSpec,
        ActionConfirmationSettledPayload,
        ActionOrigin,
        AiAction,
        AskUserOption,
        ConnectStepSnapshot,
        DataKind,
        NoteTargetSelector,
        RiskAssessment,
        RiskLevel,
        StepDetail,
        AutoReconnectChangedPayload,
        AutoReconnectPhase,
        ConfirmationSettledReason,
        DataChangedPayload,
        ConnectionStatusChangedPayload,
        ChatSessionChangedPayload,
        HostKeyKind,
        HostKeyVerificationNeededPayload,
        HostKeyPromptEndReason,
        HostKeyVerificationEndedPayload,
        AuthPromptItemPayload,
        AuthPromptNeededPayload,
        AuthPromptEndReason,
        AuthPromptEndedPayload,
        ConnectProgressPayload,
        ConnectWarning,
        ConnectWarningsPayload,
        TerminalOutputPayload,
        ChatTextDeltaPayload,
        ChatAutoContinuationStartedPayload,
        AiBudgetWaitingPayload,
        ChatActionProposedPayload,
        RiskAssessmentUpdatedPayload,
        ActionDecisionEscalatedPayload,
        NoteUpdateSuggestedPayload,
        NoteUpdateExpiredPayload,
        NoteShrinkSuggestedPayload,
        NoteShrinkFailedPayload,
        NoteShrinkSucceededPayload,
        ChatDocumentGeneratedPayload,
        ChatQuestionAskedPayload,
        ActionResultPayload,
        ChatActionResultPayload,
        ChatActionOutputPayload,
        ChatActionWaitingInputPayload,
        SessionOnlyPayload,
        ChatErrorPayload,
        ChatErrorDetails,
        ChatAiRequestStartedPayload,
        ChatAiRetryingPayload,
        ChatWebActivityPayload,
        InjectionCheckState,
        ChatInjectionCheckPayload,
        ChatAutoContinuationLimitReachedPayload,
        SftpTransferKind,
        SftpTransferStartedPayload,
        ElevationAutoDisabledPayload,
        SftpTransferFinishedPayload,
        SftpEntriesProgress,
        SftpTransferProgressPayload,
        SftpFolderProgress,
        McpActionTabRequestedPayload,
        ChatContextUsagePayload,
        ActionUserDecision,
        AiProviderConfigDto,
        AiProviderConfigInput,
        AppInfoDto,
        AuthMethodInput,
        AuthMethodKind,
        ConnectAuthMethodKind,
        AutoExecRuleDto,
        BuildType,
        ChatHistoryEntryDto,
        ChatHistoryExtensionPartDto,
        ChatHistoryRoleDto,
        ChatSessionSummaryDto,
        ConfirmationExplanationDto,
        ConnectStep,
        ConnectStepRecord,
        ConnectionStatus,
        CoveringRuleDto,
        Decision,
        DeleteGroupResult,
        DeletePreviewDto,
        DeleteServerResult,
        DeleteUnusableServerResult,
        DocumentFormat,
        DownloadResultDto,
        EditSessionDto,
        ElevationDisableDto,
        ElevationFailureDto,
        ElevationFailureKind,
        ElevationResultDto,
        ElevationWarningDto,
        ElevationWarningKind,
        Entitlements,
        EvalContextInput,
        EvaluationTraceDto,
        ExplainedSuggestionDto,
        ExtensionMedia,
        Feature,
        FeatureLocked,
        GroupDto,
        GroupId,
        HostKeyCheckResult,
        HostKeyUserDecision,
        KeepGroupResult,
        KeyFileFactsDto,
        KeyFileProblemDto,
        KeychainStatusDto,
        KeychainUnavailableReasonDto,
        LocalFilePreviewDto,
        McpSessionDto,
        NoteEditorDto,
        NoteRevisionDto,
        PatternDto,
        PatternSuggestionDto,
        PatternType,
        PostIngestPolicy,
        ProviderId,
        ProviderType,
        QuestionOptionDto,
        RemoteEntryDto,
        LinkKind,
        RuleAction,
        RuleDto,
        RuleId,
        RuleInput,
        RunningOperationDto,
        Scope,
        SegmentCoverageDto,
        SegmentCoverageState,
        ServerDto,
        ServerId,
        ServerInput,
        SessionSummaryDto,
        StepStatus,
        StoredHostKeyDto,
        SuggestionKind,
        SuggestionPatternDto,
        SuggestionVerdictDto,
        TestConnectionReport,
        TestConnectionResult,
        TierId,
        TraceKind,
        UnusableReasonDto,
        UnusableServerDto,
        VerdictReasonDto,
        WebActivityDto,
        WebActivityKindDto,
        WebSourceDto,
    ];
    // The Rust doc comments are not part of the contract (and not in English);
    // drop them so the file holds one declaration per line.
    let docs = regex::Regex::new(r"(?s)\n?/\*\*.*?\*/\n?").unwrap();
    let body = docs
        .replace_all(&decls.join("\n\n"), "")
        .replace(", \n", ", ");
    format!("{HEADER}{body}\n{}\n", event_payloads())
}

/// `EventPayloads`: event name -> payload type, for every event whose payload
/// struct lives in `app-logic` (Issue #920). Events emitted by `app-shell`
/// directly are listed by hand in the frontend (`eventPayloads.ts`).
fn event_payloads() -> String {
    use crate::event_names as names;
    let pairs: &[(&str, &str)] = &[
        (names::DATA_CHANGED, "DataChangedPayload"),
        (names::ACTIVITY_CHANGED, "Activity"),
        (names::QUIT_REQUESTED, "QuitRequestedPayload"),
        (
            names::CONNECTION_STATUS_CHANGED,
            "ConnectionStatusChangedPayload",
        ),
        (names::CHAT_SESSION_CHANGED, "ChatSessionChangedPayload"),
        (
            names::HOST_KEY_VERIFICATION_NEEDED,
            "HostKeyVerificationNeededPayload",
        ),
        (
            names::HOST_KEY_VERIFICATION_ENDED,
            "HostKeyVerificationEndedPayload",
        ),
        (names::AUTH_PROMPT_NEEDED, "AuthPromptNeededPayload"),
        (names::AUTH_PROMPT_ENDED, "AuthPromptEndedPayload"),
        (names::CONNECT_PROGRESS, "ConnectProgressPayload"),
        (names::CONNECT_WARNINGS, "ConnectWarningsPayload"),
        (names::TERMINAL_OUTPUT, "TerminalOutputPayload"),
        (names::CHAT_TEXT_DELTA, "ChatTextDeltaPayload"),
        (
            names::CHAT_AUTO_CONTINUATION_STARTED,
            "ChatAutoContinuationStartedPayload",
        ),
        (names::AI_BUDGET_WAITING, "AiBudgetWaitingPayload"),
        (
            names::RISK_ASSESSMENT_UPDATED,
            "RiskAssessmentUpdatedPayload",
        ),
        (
            names::ACTION_DECISION_ESCALATED,
            "ActionDecisionEscalatedPayload",
        ),
        (names::NOTE_UPDATE_SUGGESTED, "NoteUpdateSuggestedPayload"),
        (names::NOTE_UPDATE_EXPIRED, "NoteUpdateExpiredPayload"),
        (names::NOTE_SHRINK_SUGGESTED, "NoteShrinkSuggestedPayload"),
        (names::NOTE_SHRINK_FAILED, "NoteShrinkFailedPayload"),
        (names::NOTE_SHRINK_SUCCEEDED, "NoteShrinkSucceededPayload"),
        (
            names::CHAT_DOCUMENT_GENERATED,
            "ChatDocumentGeneratedPayload",
        ),
        (names::CHAT_QUESTION_ASKED, "ChatQuestionAskedPayload"),
        (names::CHAT_ACTION_RESULT, "ChatActionResultPayload"),
        (names::CHAT_ACTION_OUTPUT, "ChatActionOutputPayload"),
        (
            names::CHAT_ACTION_WAITING_INPUT,
            "ChatActionWaitingInputPayload",
        ),
        (names::CHAT_ERROR, "ChatErrorPayload"),
        (names::CHAT_RESPONSE_TRUNCATED, "SessionOnlyPayload"),
        (
            names::CHAT_AI_REQUEST_STARTED,
            "ChatAiRequestStartedPayload",
        ),
        (names::CHAT_AI_RETRYING, "ChatAiRetryingPayload"),
        (names::CHAT_WEB_ACTIVITY, "ChatWebActivityPayload"),
        (names::CHAT_INJECTION_CHECK, "ChatInjectionCheckPayload"),
        (names::CHAT_WEB_RESEARCH_UNAVAILABLE, "SessionOnlyPayload"),
        (names::CHAT_RESPONSE_EMPTY, "SessionOnlyPayload"),
        (names::CHAT_RESPONSE_REFUSED, "SessionOnlyPayload"),
        (names::CHAT_RESPONSE_CANCELLED, "SessionOnlyPayload"),
        (names::CHAT_QUEUED_MESSAGES_SENT, "SessionOnlyPayload"),
        (
            names::CHAT_AUTO_CONTINUATION_LIMIT_REACHED,
            "ChatAutoContinuationLimitReachedPayload",
        ),
        (names::SFTP_TRANSFER_STARTED, "SftpTransferStartedPayload"),
        (
            names::ELEVATION_AUTO_DISABLED,
            "ElevationAutoDisabledPayload",
        ),
        (names::ELEVATION_REVOKED_BY_PROFILE, "SessionOnlyPayload"),
        (names::SFTP_TRANSFER_FINISHED, "SftpTransferFinishedPayload"),
        (names::SFTP_TRANSFER_PROGRESS, "SftpTransferProgressPayload"),
        (
            names::MCP_ACTION_TAB_REQUESTED,
            "McpActionTabRequestedPayload",
        ),
        (names::CHAT_CONTEXT_USAGE, "ChatContextUsagePayload"),
        (names::CHAT_CONTEXT_COMPACTED, "SessionOnlyPayload"),
        (names::CHAT_ACTION_PROPOSED, "ChatActionProposedPayload"),
        (names::AUTO_RECONNECT_CHANGED, "AutoReconnectChangedPayload"),
        (
            names::ACTION_CONFIRMATION_SETTLED,
            "ActionConfirmationSettledPayload",
        ),
        (names::ENTITLEMENTS_CHANGED, "Entitlements"),
    ];
    let mut pairs = pairs.to_vec();
    pairs.sort();
    let lines: Vec<String> = pairs
        .iter()
        .map(|(name, ty)| format!("  \"{name}\": {ty},"))
        .collect();
    format!("export type EventPayloads = {{\n{}\n}};", lines.join("\n"))
}

fn target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/smart-ssh-community/frontend/src/generated/ipc.ts")
}

#[test]
fn export_bindings() {
    let generated = generate();
    let path = target();
    if std::env::var_os("UPDATE_IPC_TS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == generated,
        "src/generated/ipc.ts is out of date; run `npm run gen:ipc` in apps/smart-ssh-community/frontend"
    );
}
