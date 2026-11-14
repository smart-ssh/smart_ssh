// generated – do not edit.
// Regenerate with `npm run gen:ipc` (apps/smart-ssh-community/frontend).
// Source: the Rust IPC types in `app-logic` and `ssh_manager_core`.

export type ActionConfirmationSettledPayload = { sessionId: string, actionId: string, reason: ConfirmationSettledReason, };

export type ActionDecisionEscalatedPayload = { sessionId: string, actionId: string, reason: string, code: string, };

export type ActionOrigin = { "kind": "internal" } | { "kind": "mcp", clientName: string | null, };

export type ActionResultPayload = { "kind": "command", command: string, stdout: string, stderr: string, exitCode: number | null, cancelled: boolean, truncated: boolean, } | { "kind": "noteUpdate", summary: string, } | { "kind": "fileRead", path: string, content: string, } | { "kind": "fileWrite", path: string, backupPath: string | null, usedSudoPassword: boolean, };

export type ActionUserDecision = { "decision": "approve" } | { "decision": "deny" } | { "decision": "editThenApprove", command: string, };

export type AiAction = { "SuggestCommand": { command: string, } } | { "ProposeNoteUpdate": { target: NoteTargetSelector, new_content: string, } } | { "GenerateDocument": { title: string, content_markdown: string, } } | { "ReadRemoteFile": { path: string, } } | { "WriteRemoteFile": { path: string, content: string, } } | { "AskUser": { question: string, options: Array<AskUserOption>, multi_select: boolean, } };

export type AiBudgetWaitingPayload = { sessionId: string, waitSeconds: number, };

export type AiProviderConfigDto = { id: ProviderId, providerType: ProviderType, displayName: string, baseUrl: string | null, model: string, supportsNativeToolCalling: boolean, isActive: boolean, extraHeaders: Array<[string, string]>, attestationUrl: string | null, maxTokensOverride: number | null, webResearchEnabled: boolean, };

export type AiProviderConfigInput = { providerType: ProviderType, displayName: string, baseUrl: string | null, model: string, supportsNativeToolCalling: boolean, apiKey: string, extraHeaders: Array<[string, string]>, attestationUrl: string | null, maxTokensOverride: number | null, webResearchEnabled: boolean, };

export type AppInfoDto = { version: string, commitHash: string, versionDisplay: string, edition: string, buildType: BuildType, dataDir: string | null, dataDirDisplay: string | null, settingsStorePath: string | null, };

export type AskUserOption = { label: string, description?: string | null, };

export type AuthMethodInput = { "kind": "password", value: string | null, } | { "kind": "noPassword" } | { "kind": "privateKey", keyContent: string | null, passphrase: string | null, } | { "kind": "agent" } | { "kind": "certificate", certContent: string | null, keyContent: string | null, } | { "kind": "identityFile", path: string, passphrase: string | null, };

export type AuthMethodKind = "password" | "no_password" | "private_key" | "agent" | "certificate" | "identity_file";

export type AuthPromptEndReason = "answered" | "cancelled" | "timed_out" | "abandoned";

export type AuthPromptEndedPayload = { promptId: string, reason: AuthPromptEndReason, };

export type AuthPromptItemPayload = { text: string, echo: boolean, };

export type AuthPromptNeededPayload = { promptId: string, connectId: string, hop: string, kind: string, name: string, instructions: string, prompts: Array<AuthPromptItemPayload>, };

export type AutoExecRuleDto = { ruleId: RuleId, patternType: PatternType, patternValue: string, scope: Scope, segmentIndices: Array<number>, };

export type AutoReconnectChangedPayload = { sessionId: string, phase: AutoReconnectPhase, attempt: number, };

export type AutoReconnectPhase = "running" | "reconnected" | "gave_up" | "stopped" | "blocked";

export type BuildType = "Dev" | "Release";

export type ChatActionOutputPayload = { sessionId: string, actionId: string, stdout: string, stderr: string, truncated: boolean, };

export type ChatActionProposedPayload = { sessionId: string, actionId: string, action: AiAction, decision: Decision, previousNoteContent: string | null, usesStoredSudoPassword: boolean, previousFileContent: string | null, previousFileSize: number | null, targetName: string | null, riskAssessment: RiskAssessment | null, origin: ActionOrigin, autoExecRules: Array<AutoExecRuleDto>, };

export type ChatActionResultPayload = { sessionId: string, actionId: string, result: ActionResultPayload, };

export type ChatActionWaitingInputPayload = { sessionId: string, actionId: string, waiting: boolean, };

export type ChatAiRequestStartedPayload = { sessionId: string, providerName: string, };

export type ChatAiRetryingPayload = { sessionId: string, attempt: number, maxAttempts: number, delaySecs: number, };

export type ChatAutoContinuationLimitReachedPayload = { sessionId: string, limit: number, };

export type ChatAutoContinuationStartedPayload = { sessionId: string, round: number, };

export type ChatContextUsagePayload = { sessionId: string, inputTokens: number, windowTokens: number, };

export type ChatDocumentGeneratedPayload = { sessionId: string, actionId: string, title: string, contentMarkdown: string, };

export type ChatErrorDetails = { providerErrorType: string | null, httpStatus: number | null, providerMessage: string | null, providerName: string | null, timeoutSecs: number | null, accessReturns: string | null, };

export type ChatErrorPayload = { sessionId: string, message: string, code: string | null, providerErrorType: string | null, httpStatus: number | null, providerMessage: string | null, providerName: string | null, timeoutSecs: number | null, accessReturns: string | null, };

export type ChatHistoryEntryDto = { "type": "text", role: ChatHistoryRoleDto, text: string, } | { "type": "commandResult", role: ChatHistoryRoleDto, command: string, stdout: string, stderr: string, exitCode: number | null, cancelled: boolean, truncated: boolean, } | { "type": "actionRejected", role: ChatHistoryRoleDto, command: string, reason: string, } | { "type": "webActivity", role: ChatHistoryRoleDto, activity: WebActivityDto, } | { "type": "extensionParts", role: ChatHistoryRoleDto, items: Array<ChatHistoryExtensionPartDto>, } | { "type": "question", role: ChatHistoryRoleDto, questionId: string, question: string, options: Array<QuestionOptionDto>, multiSelect: boolean, };

export type ChatHistoryExtensionPartDto = { label: string, media: ExtensionMedia, };

export type ChatHistoryRoleDto = "user" | "assistant" | "action_result";

export type ChatInjectionCheckPayload = { sessionId: string, itemId: string, } & ({ "state": "running" } | { "state": "clean" } | { "state": "suspected", reason: string, } | { "state": "unavailable" });

export type ChatQuestionAskedPayload = { sessionId: string, questionId: string, question: string, options: Array<QuestionOptionDto>, multiSelect: boolean, };

export type ChatSessionChangedPayload = { sessionId: string, chatSessionId: string | null, };

export type ChatSessionSummaryDto = { sessionId: string, title: string | null, startedAt: string, endedAt: string | null, messageCount: number, };

export type ChatTextDeltaPayload = { sessionId: string, delta: string, };

export type ChatWebActivityPayload = { sessionId: string, activityId: string, activity: WebActivityDto, };

export type ConfirmationExplanationDto = { segments: Array<SegmentCoverageDto>, suggestions: Array<ExplainedSuggestionDto>, };

export type ConfirmationSettledReason = "reconnect" | "timeout";

export type ConnectAuthMethodKind = "none" | "password" | "privateKey" | "agent" | "certificate" | "identityFile" | "keyboardInteractive";

export type ConnectProgressPayload = { attemptId: string, serverId: ServerId, attempt: number, steps: Array<ConnectStepSnapshot>, };

export type ConnectStep = { "kind": "dnsResolution", host: string, port: number, addresses: Array<string>, } | { "kind": "tcpConnect", address: string | null, port: number, } | { "kind": "tunnelOpen", host: string, port: number, } | { "kind": "handshake", serverVersion: string | null, kex: string | null, hostKeyAlgorithm: string | null, cipher: string | null, mac: string | null, } | { "kind": "hostKeyCheck", keyType: string | null, fingerprint: string | null, result: HostKeyCheckResult | null, } | { "kind": "authentication", method: ConnectAuthMethodKind, remainingMethods: Array<string>, partialSuccess: boolean, round: number | null, prompts: Array<string>, } | { "kind": "sessionReady" } | { "kind": "outdatedAlgorithms", algorithms: Array<string>, } | { "kind": "banner", text: string, };

export type ConnectStepRecord = { hopIndex: number, hop: string, step: ConnectStep, status: StepStatus, durationMs: number | null, };

export type ConnectStepSnapshot = { kind: "dnsResolution" | "tcpConnect" | "tunnelOpen" | "handshake" | "hostKeyCheck" | "authentication" | "sessionReady" | "outdatedAlgorithms" | "banner", hopIndex: number, hop: string, status: "running" | "ok" | "skipped" | "warning" | "failed", errorCode: string | null, detail: StepDetail | null, };

export type ConnectWarning = { hop: string, algorithms: Array<string>, };

export type ConnectWarningsPayload = { sessionId: string, warnings: Array<ConnectWarning>, hopCount: number, };

export type ConnectionStatus = "connected" | "disconnected" | "reconnecting" | "awaiting_host_key";

export type ConnectionStatusChangedPayload = { sessionId: string, status: ConnectionStatus, reason: string | null, };

export type CoveringRuleDto = { patternType: PatternType, patternValue: string, };

export type DataChangedPayload = { kinds: Array<DataKind>, };

export type DataKind = "servers" | "groups" | "tags" | "rules" | "notes" | "ai_providers" | "secrets" | "host_keys";

export type Decision = "AutoExec" | { "Confirm": { reason: string, code: string, } } | { "Deny": { reason: string, code: string, } };

export type DeleteGroupResult = { childGroupsToDelete: Array<GroupDto>, serversToUnassign: Array<ServerDto>, unusableServersToUnassign: Array<UnusableServerDto>, executed: boolean, };

export type DeletePreviewDto = { fileCount: number, dirCount: number, };

export type DeleteServerResult = { server: ServerDto, serversLosingJumpHost: Array<ServerDto>, executed: boolean, secretsLeftBehind: Array<string>, };

export type DeleteUnusableServerResult = { server: UnusableServerDto, serversLosingJumpHost: Array<ServerDto>, executed: boolean, secretsLeftBehind: Array<string>, };

export type DocumentFormat = "markdown";

export type DownloadResultDto = { localPath: string, isDir: boolean, fileCount: number, };

export type EditSessionDto = { localPath: string, remoteModified: string | null, };

export type ElevationAutoDisabledPayload = { sessionId: string, minutes: number, };

export type ElevationDisableDto = { disabled: boolean, runningOperations: Array<RunningOperationDto>, };

export type ElevationFailureDto = { kind: ElevationFailureKind, sudoersLine: string | null, detail: string | null, };

export type ElevationFailureKind = "unsupported" | "invalidUser" | "invalidPath" | "sftpServerNotFound" | "sftpServerUnsafe" | "passwordRequired" | "wrongSudoPassword" | "notAllowed" | "requireTty" | "sudoMissing" | "checkFailed" | "startFailed" | "disabledForServer";

export type ElevationResultDto = { active: boolean, targetUser: string, sftpServerPath: string | null, failure: ElevationFailureDto | null, warning: ElevationWarningDto | null, runningOperations: Array<RunningOperationDto>, serverLogging: boolean, sudoAuth: string | null, };

export type ElevationWarningDto = { kind: ElevationWarningKind, path: string, };

export type ElevationWarningKind = "sftpServerUnsafe";

export type Entitlements = { tier: TierId, features: Array<Feature>, seats: number | null, expiresAt: string | null, nonCommercial: boolean, licensee: string | null, };

export type EvalContextInput = { serverId: ServerId | null, tags: Array<string>, };

export type EvaluationTraceDto = { decision: Decision, matchedRule: RuleId | null, matchedHardBlacklistEntry: string | null, subCommandTraces: Array<EvaluationTraceDto>, kind: TraceKind, segmentIndex: number | null, checks: Array<string>, };

export type ExplainedSuggestionDto = { verdict: SuggestionVerdictDto, kind: SuggestionKind, names: Array<string>, patterns: Array<SuggestionPatternDto>, };

export type ExtensionMedia = "text" | "image" | "pdf";

export type Feature = string;

export type FeatureLocked = { feature: Feature, tier: TierId, };

export type GroupDto = { id: string, name: string, parentId: string | null, notes: string, isCopy: boolean, };

export type GroupId = string;

export type HostKeyCheckResult = "known" | "unknown" | "changed";

export type HostKeyKind = "unknown" | "mismatch";

export type HostKeyPromptEndReason = "decided" | "timed_out" | "abandoned";

export type HostKeyUserDecision = { "decision": "trust" } | { "decision": "reject" };

export type HostKeyVerificationEndedPayload = { sessionId: string, promptId: number, reason: HostKeyPromptEndReason, };

export type HostKeyVerificationNeededPayload = { sessionId: string, promptId: number, host: string, port: number, kind: HostKeyKind, fingerprint: string, expectedFingerprint: string | null, keyType: string | null, };

export type InjectionCheckState = { "state": "running" } | { "state": "clean" } | { "state": "suspected", reason: string, } | { "state": "unavailable" };

export type KeepGroupResult = { applied: boolean, groups: number, servers: number, };

export type KeyFileFactsDto = { exists: boolean, permissionsTooOpen: boolean, validKey: boolean, encrypted: boolean, problem: KeyFileProblemDto | null, };

export type KeyFileProblemDto = { code: string, message: string, };

export type KeychainStatusDto = { available: boolean, reason: KeychainUnavailableReasonDto | null, };

export type KeychainUnavailableReasonDto = "no_session_bus" | "no_secret_service_provider" | "locked" | "no_keystore" | "unknown";

export type LinkKind = "dir" | "file" | "broken";

export type LocalFilePreviewDto = { text: string | null, size: number, };

export type McpActionTabRequestedPayload = { sessionId: string, serverId: ServerId, clientName: string | null, };

export type McpSessionDto = { clientName: string | null, };

export type NoteEditorDto = { "kind": "user" } | { "kind": "ai", provider: string, model: string, };

export type NoteRevisionDto = { id: string, content: string, editedBy: NoteEditorDto, createdAt: string, };

export type NoteShrinkFailedPayload = { serverId: ServerId, message: string, };

export type NoteShrinkSucceededPayload = { serverId: ServerId, };

export type NoteShrinkSuggestedPayload = { serverId: ServerId, serverName: string, };

export type NoteTargetSelector = "CurrentServer" | "CurrentServerGroup";

export type NoteUpdateExpiredPayload = { actionId: string, };

export type NoteUpdateSuggestedPayload = { sessionId: string, actionId: string, action: AiAction, previousNoteContent: string | null, targetName: string | null, summaryIncomplete: boolean, };

export type PatternDto = { kind: string, value: string, };

export type PatternSuggestionDto = { kind: SuggestionKind, names: Array<string>, patterns: Array<SuggestionPatternDto>, };

export type PatternType = "glob" | "regex" | "exact";

export type PostIngestPolicy = "strict" | "balanced" | "standard";

export type ProviderId = string;

export type ProviderType = "openai" | "anthropic" | "generic_openai_compatible" | "ollama";

export type QuestionOptionDto = { label: string, description: string | null, };

export type RemoteEntryDto = { name: string, path: string, isDir: boolean, size: number, permissions: string, modified: string | null, permissionsOctal: number, uid: number | null, gid: number | null, owner: string | null, group: string | null, isSymlink: boolean, linkTarget: string | null, linkKind: LinkKind | null, };

export type RiskAssessment = { serverRisk: RiskLevel, serverRiskReason: string | null, dataRisk: RiskLevel, dataRiskReason: string | null, aiReviewed: boolean, };

export type RiskAssessmentUpdatedPayload = { sessionId: string, actionId: string, dataRisk: RiskLevel, reason: string | null, secondOpinionUnavailable: boolean, };

export type RiskLevel = "none" | "yellow" | "red";

export type RuleAction = "Allow" | "Confirm" | "Deny";

export type RuleDto = { id: RuleId, patternType: PatternType, patternValue: string, action: RuleAction, scope: Scope, priority: number, patternError: string | null, };

export type RuleId = string;

export type RuleInput = { patternType: PatternType, patternValue: string, action: RuleAction, scope: Scope, priority: number, };

export type RunningOperationDto = { id: string, kind: string, path: string, bytesDone: number | null, bytesTotal: number | null, filesDone: number | null, filesTotal: number | null, };

export type Scope = "Global" | { "Server": ServerId } | { "Tag": string };

export type SegmentCoverageDto = { index: number, text: string, state: SegmentCoverageState, rule: CoveringRuleDto | null, reasons: Array<string>, };

export type SegmentCoverageState = "covered" | "needsRule" | "blocked";

export type ServerDto = { id: string, name: string, host: string, port: number, username: string, groupId: string | null, tags: Array<string>, authKind: AuthMethodKind, identityFilePath: string | null, jumpHost: string | null, notes: string, hasSudoPassword: boolean, sudoPasswordUnknown: boolean, isLocal: boolean, postIngestPolicy: PostIngestPolicy, aiInjectionCheckEnabled: boolean, sftpServerPath: string | null, startDirectory: string | null, elevationAllowed: boolean, isCopy: boolean, };

export type ServerId = string;

export type ServerInput = { name: string, host: string, port: number, username: string, groupId: GroupId | null, tags: Array<string>, auth: AuthMethodInput, jumpHost: ServerId | null, sudoPassword: string | null, postIngestPolicy: PostIngestPolicy, aiInjectionCheckEnabled: boolean, sftpServerPath: string | null, startDirectory: string | null, elevationAllowed: boolean, };

export type SessionOnlyPayload = { sessionId: string, };

export type SessionSummaryDto = { sessionId: string, serverId: ServerId, serverName: string, status: ConnectionStatus, hasPendingAction: boolean, chatSessionId: string | null, mcp: McpSessionDto | null, };

export type SftpEntriesProgress = { done: number, total: number | null, };

export type SftpFolderProgress = { filesDone: number, filesTotal: number, bytesDone: number, bytesTotal: number, };

export type SftpTransferFinishedPayload = { sessionId: string, transferId: string, error: string | null, stopped: "cancelled" | "stopped" | null, incompleteRemotePath: string | null, entriesDone: number | null, };

export type SftpTransferKind = "upload" | "download" | "delete" | "chmod" | "chown";

export type SftpTransferProgressPayload = { sessionId: string, transferId: string, bytesDone: number, totalBytes: number | null, folder: SftpFolderProgress | null, entries: SftpEntriesProgress | null, };

export type SftpTransferStartedPayload = { sessionId: string, transferId: string, kind: SftpTransferKind, fileName: string, totalBytes: number | null, };

export type StepDetail = { "code": "contacting", target: string, } | { "code": "tunnel", hop: string, } | { "code": "waitingForResponse" } | { "code": "waitingForDecision" } | { "code": "authMethod", method: string, };

export type StepStatus = { "state": "running" } | { "state": "ok" } | { "state": "skipped" } | { "state": "warning", code: string, } | { "state": "failed", code: string, };

export type StoredHostKeyDto = { algorithm: string, fingerprint: string, };

export type SuggestionKind = "exact" | "exactPerSegment" | "pathScoped" | "lastArgOpen" | "subcommand" | "tool";

export type SuggestionPatternDto = { patternType: PatternType, patternValue: string, };

export type SuggestionVerdictDto = { autoExec: boolean, reasons: Array<VerdictReasonDto>, };

export type TerminalOutputPayload = { sessionId: string, data: string, };

export type TestConnectionReport = { steps: Array<ConnectStepRecord>, } & ({ "kind": "success" } | { "kind": "authenticationFailed" } | { "kind": "hostKeyUnknown", host: string, port: number, rawKey: Array<number>, fingerprint: string, keyType: string | null, } | { "kind": "hostKeyMismatch", host: string, port: number, rawKey: Array<number>, expectedFingerprint: string, actualFingerprint: string, keyType: string | null, } | { "kind": "networkError", message: string, code: string | null, } | { "kind": "timeout" });

export type TestConnectionResult = { "kind": "success" } | { "kind": "authenticationFailed" } | { "kind": "hostKeyUnknown", host: string, port: number, rawKey: Array<number>, fingerprint: string, keyType: string | null, } | { "kind": "hostKeyMismatch", host: string, port: number, rawKey: Array<number>, expectedFingerprint: string, actualFingerprint: string, keyType: string | null, } | { "kind": "networkError", message: string, code: string | null, } | { "kind": "timeout" };

export type TierId = string;

export type TraceKind = "Segment" | "Substitution" | "EmbeddedCode";

export type UnusableReasonDto = "unknown_auth_method" | "unreadable_auth_method";

export type UnusableServerDto = { id: ServerId, name: string, host: string, groupId: GroupId | null, reason: UnusableReasonDto, };

export type VerdictReasonDto = { code: string, segmentIndex: number | null, };

export type WebActivityDto = { kind: WebActivityKindDto, input: string, results: Array<WebSourceDto>, cited: Array<WebSourceDto>, contentTruncated: boolean, errorCode: string | null, };

export type WebActivityKindDto = "search" | "fetch";

export type WebSourceDto = { title: string, url: string, };
export type EventPayloads = {
  "action-confirmation-settled": ActionConfirmationSettledPayload,
  "action-decision-escalated": ActionDecisionEscalatedPayload,
  "ai-budget-waiting": AiBudgetWaitingPayload,
  "auth-prompt-ended": AuthPromptEndedPayload,
  "auth-prompt-needed": AuthPromptNeededPayload,
  "auto-reconnect-changed": AutoReconnectChangedPayload,
  "chat-action-output": ChatActionOutputPayload,
  "chat-action-proposed": ChatActionProposedPayload,
  "chat-action-result": ChatActionResultPayload,
  "chat-action-waiting-input": ChatActionWaitingInputPayload,
  "chat-ai-request-started": ChatAiRequestStartedPayload,
  "chat-ai-retrying": ChatAiRetryingPayload,
  "chat-auto-continuation-limit-reached": ChatAutoContinuationLimitReachedPayload,
  "chat-auto-continuation-started": ChatAutoContinuationStartedPayload,
  "chat-context-compacted": SessionOnlyPayload,
  "chat-context-usage": ChatContextUsagePayload,
  "chat-document-generated": ChatDocumentGeneratedPayload,
  "chat-error": ChatErrorPayload,
  "chat-injection-check": ChatInjectionCheckPayload,
  "chat-question-asked": ChatQuestionAskedPayload,
  "chat-queued-messages-sent": SessionOnlyPayload,
  "chat-response-cancelled": SessionOnlyPayload,
  "chat-response-empty": SessionOnlyPayload,
  "chat-response-refused": SessionOnlyPayload,
  "chat-response-truncated": SessionOnlyPayload,
  "chat-session-changed": ChatSessionChangedPayload,
  "chat-text-delta": ChatTextDeltaPayload,
  "chat-web-activity": ChatWebActivityPayload,
  "chat-web-research-unavailable": SessionOnlyPayload,
  "connect-progress": ConnectProgressPayload,
  "connect-warnings": ConnectWarningsPayload,
  "connection-status-changed": ConnectionStatusChangedPayload,
  "data-changed": DataChangedPayload,
  "elevation-auto-disabled": ElevationAutoDisabledPayload,
  "elevation-revoked-by-profile": SessionOnlyPayload,
  "entitlements:changed": Entitlements,
  "host-key-verification-ended": HostKeyVerificationEndedPayload,
  "host-key-verification-needed": HostKeyVerificationNeededPayload,
  "mcp-action-tab-requested": McpActionTabRequestedPayload,
  "note-shrink-failed": NoteShrinkFailedPayload,
  "note-shrink-succeeded": NoteShrinkSucceededPayload,
  "note-shrink-suggested": NoteShrinkSuggestedPayload,
  "note-update-expired": NoteUpdateExpiredPayload,
  "note-update-suggested": NoteUpdateSuggestedPayload,
  "risk-assessment-updated": RiskAssessmentUpdatedPayload,
  "sftp-transfer-finished": SftpTransferFinishedPayload,
  "sftp-transfer-progress": SftpTransferProgressPayload,
  "sftp-transfer-started": SftpTransferStartedPayload,
  "terminal-output": TerminalOutputPayload,
};
