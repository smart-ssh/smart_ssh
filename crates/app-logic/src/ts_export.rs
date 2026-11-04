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
    use crate::dto::*;
    use crate::status::ConnectionStatus;
    use crate::version::BuildType;
    use ssh_manager_core::ai::{ExtensionMedia, ProviderId, ProviderType};
    use ssh_manager_core::entitlements::{Entitlements, Feature, FeatureLocked, TierId};
    use ssh_manager_core::filter::{Decision, RuleAction, RuleId, Scope, TraceKind};
    use ssh_manager_core::profiles::{GroupId, PostIngestPolicy};
    use ssh_manager_core::shared::ServerId;
    use ssh_manager_core::ssh::connect_log::{
        AuthMethodKind as ConnectAuthMethodKind, ConnectStep, ConnectStepRecord,
        HostKeyCheckResult, StepStatus,
    };
    let decls = decls![
        ActionOrigin,
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
    format!("{HEADER}{body}\n")
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
