// IPC types come from the Rust definitions: `generated/ipc.ts` is written by
// `npm run gen:ipc` (issue #919). This file re-exports them under their
// existing names and keeps only the hand-written helpers and the types that do
// not cross the IPC boundary as a DTO.

import type {
  HostKeyKind,
  PatternType,
  ProviderType,
  Scope,
} from "../generated/ipc";

export type {
  ActionOrigin,
  ActionUserDecision,
  AiProviderConfigDto,
  AiProviderConfigInput,
  AppInfoDto,
  AuthMethodInput,
  AuthMethodKind,
  AutoExecRuleDto,
  BuildType,
  ChatHistoryEntryDto,
  ChatHistoryExtensionPartDto,
  ChatHistoryRoleDto,
  ChatSessionSummaryDto,
  ConfirmationExplanationDto,
  ConnectAuthMethodKind,
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
} from "../generated/ipc";

/** Spec 0105: nur diese Provider-Typen haben serverseitige Web-Werkzeuge —
 * nur für sie zeigt das Formular den Schalter. */
export function supportsWebResearch(type: ProviderType, baseUrl?: string | null): boolean {
  if (type === "anthropic") return true;
  // OpenAI: nur der offizielle Dienst (keine eigene Basis-URL); gleiche Regel
  // wie `is_official_openai` im Backend.
  if (type === "openai") {
    const url = baseUrl?.trim().replace(/\/+$/, "") ?? "";
    return url === "" || url === "https://api.openai.com/v1";
  }
  return false;
}

// Nur bei diesen beiden Typen ist Base-URL relevant (Spec 0007, Abschnitt
// 8.3: "Base-URL-Feld nur bei generic_openai_compatible/ollama sichtbar").
export function needsBaseUrl(type: ProviderType): boolean {
  return type === "generic_openai_compatible" || type === "ollama";
}

// Spec 0072, B2/B3: `discover_models` funktioniert gegen alle vier
// Provider-Typen — `anthropic` hat, anders als in Spec 0025 Abschnitt 2
// angenommen, ein äquivalentes Endpoint-Verhalten (Spec 0072 §1), nur unter
// `GET {base_url}/v1/models` mit `x-api-key`/`anthropic-version` statt
// `Authorization: Bearer` gegen `GET {base_url}/models` bei der
// OpenAI-Familie (Pfad und Header vom Backend gewählt, s.
// `crates/ai-providers/src/discovery.rs`, Spec-Reviewer-Fund: Anthropics
// `base_url` trägt anders als bei der OpenAI-Familie kein `/v1`-Präfix).
// Weiterhin als Funktion (statt schlicht `true` überall zu verwenden) für
// den Fall eines künftigen `ProviderType`, der das nicht unterstützt.
export function supportsModelDiscovery(type: ProviderType): boolean {
  return (
    type === "openai" ||
    type === "anthropic" ||
    type === "generic_openai_compatible" ||
    type === "ollama"
  );
}

// Event payloads and their helper types: generated from the Rust payload
// structs (issue #920). The `*Event` names are kept as aliases.
export type {
  ActionDecisionEscalatedPayload,
  ActionResultPayload,
  AiAction,
  AiBudgetWaitingPayload,
  AutoReconnectChangedPayload,
  ChatActionOutputPayload,
  ChatActionProposedPayload,
  ChatActionResultPayload,
  ChatActionWaitingInputPayload,
  ChatAiRequestStartedPayload,
  ChatAiRetryingPayload,
  ChatAutoContinuationLimitReachedPayload,
  ChatAutoContinuationStartedPayload,
  ChatContextUsagePayload,
  ChatDocumentGeneratedPayload,
  ChatErrorPayload,
  ChatInjectionCheckPayload,
  ChatQuestionAskedPayload,
  ChatSessionChangedPayload,
  ChatTextDeltaPayload,
  ChatWebActivityPayload,
  AuthPromptEndReason,
  AuthPromptEndedPayload,
  AuthPromptItemPayload,
  AuthPromptNeededPayload,
  ConnectProgressPayload,
  ConnectStepSnapshot,
  ConnectionStatusChangedPayload,
  HostKeyKind,
  HostKeyPromptEndReason,
  HostKeyVerificationEndedPayload,
  HostKeyVerificationNeededPayload,
  InjectionCheckState,
  McpActionTabRequestedPayload,
  NoteShrinkFailedPayload,
  NoteShrinkSucceededPayload,
  NoteShrinkSuggestedPayload,
  NoteTargetSelector,
  NoteUpdateExpiredPayload,
  NoteUpdateSuggestedPayload,
  RiskAssessment,
  RiskAssessmentUpdatedPayload,
  RiskLevel,
  SessionOnlyPayload,
  SftpTransferFinishedPayload,
  SftpTransferKind,
  SftpTransferProgressPayload,
  ElevationAutoDisabledPayload,
  SftpTransferStartedPayload,
  StepDetail,
  TerminalOutputPayload,
} from "../generated/ipc";
import type {
  ActionDecisionEscalatedPayload,
  AiBudgetWaitingPayload,
  AutoReconnectChangedPayload,
  ChatActionOutputPayload,
  ChatActionProposedPayload,
  ChatActionResultPayload,
  ChatActionWaitingInputPayload,
  ChatAiRequestStartedPayload,
  ChatAiRetryingPayload,
  ChatAutoContinuationLimitReachedPayload,
  ChatAutoContinuationStartedPayload,
  ChatContextUsagePayload,
  ChatDocumentGeneratedPayload,
  ChatErrorPayload,
  ChatInjectionCheckPayload,
  ChatQuestionAskedPayload,
  ChatSessionChangedPayload,
  ChatTextDeltaPayload,
  ChatWebActivityPayload,
  AuthPromptEndedPayload,
  AuthPromptNeededPayload,
  ConnectProgressPayload,
  ConnectionStatusChangedPayload,
  HostKeyVerificationEndedPayload,
  HostKeyVerificationNeededPayload,
  McpActionTabRequestedPayload,
  NoteShrinkFailedPayload,
  NoteShrinkSucceededPayload,
  NoteShrinkSuggestedPayload,
  NoteUpdateExpiredPayload,
  NoteUpdateSuggestedPayload,
  RiskAssessmentUpdatedPayload,
  SessionOnlyPayload,
  SftpTransferFinishedPayload,
  SftpTransferProgressPayload,
  ElevationAutoDisabledPayload,
  SftpTransferStartedPayload,
  StepDetail,
  TerminalOutputPayload,
} from "../generated/ipc";
export type ChatSessionChangedEvent = ChatSessionChangedPayload;
export type ConnectionStatusChangedEvent = ConnectionStatusChangedPayload;
export type AutoReconnectChangedEvent = AutoReconnectChangedPayload;
export type AuthPromptNeededEvent = AuthPromptNeededPayload;
export type AuthPromptEndedEvent = AuthPromptEndedPayload;
export type HostKeyVerificationNeededEvent = HostKeyVerificationNeededPayload;
export type HostKeyVerificationEndedEvent = HostKeyVerificationEndedPayload;
export type ConnectProgressEvent = ConnectProgressPayload;
export type TerminalOutputEvent = TerminalOutputPayload;
export type ChatTextDeltaEvent = ChatTextDeltaPayload;
export type ChatActionProposedEvent = ChatActionProposedPayload;
export type RiskAssessmentUpdatedEvent = RiskAssessmentUpdatedPayload;
export type ActionDecisionEscalatedEvent = ActionDecisionEscalatedPayload;
export type NoteUpdateSuggestedEvent = NoteUpdateSuggestedPayload;
export type NoteShrinkSuggestedEvent = NoteShrinkSuggestedPayload;
export type NoteShrinkFailedEvent = NoteShrinkFailedPayload;
export type NoteShrinkSucceededEvent = NoteShrinkSucceededPayload;
export type ChatAutoContinuationStartedEvent = ChatAutoContinuationStartedPayload;
export type AiBudgetWaitingEvent = AiBudgetWaitingPayload;
export type ChatInjectionCheckEvent = ChatInjectionCheckPayload;
export type ChatActionResultEvent = ChatActionResultPayload;
export type ChatActionOutputEvent = ChatActionOutputPayload;
export type ChatActionWaitingInputEvent = ChatActionWaitingInputPayload;
export type ChatErrorEvent = ChatErrorPayload;
export type ChatAiRequestStartedEvent = ChatAiRequestStartedPayload;
export type ChatAiRetryingEvent = ChatAiRetryingPayload;
export type ChatAutoContinuationLimitReachedEvent = ChatAutoContinuationLimitReachedPayload;
export type ChatResponseTruncatedEvent = SessionOnlyPayload;
export type ChatContextUsageEvent = ChatContextUsagePayload;
export type ChatContextCompactedEvent = SessionOnlyPayload;
export type ChatWebActivityEvent = ChatWebActivityPayload;
export type ChatResponseEmptyEvent = SessionOnlyPayload;
export type ChatResponseRefusedEvent = SessionOnlyPayload;
export type ChatWebResearchUnavailableEvent = SessionOnlyPayload;
export type ChatResponseCancelledEvent = SessionOnlyPayload;
export type ChatQueuedMessagesSentEvent = SessionOnlyPayload;
export type ChatDocumentGeneratedEvent = ChatDocumentGeneratedPayload;
export type ChatQuestionAskedEvent = ChatQuestionAskedPayload;
export type SftpTransferStartedEvent = SftpTransferStartedPayload;
export type ElevationAutoDisabledEvent = ElevationAutoDisabledPayload;
export type SftpTransferFinishedEvent = SftpTransferFinishedPayload;
export type SftpTransferProgressEvent = SftpTransferProgressPayload;
export type McpActionTabRequestedEvent = McpActionTabRequestedPayload;
export type NoteUpdateExpiredEvent = NoteUpdateExpiredPayload;
export type ConnectStepDetail = StepDetail;

// --- Teil 2: Session/Terminal/Chat ------------------------------------
//
// `AiAction`/`NoteTarget`/`Decision` (core::profiles/core::filter) tragen
// kein `#[serde(rename_all/rename)]` — sie serialisieren mit Serdes
// Standard-Außen-Tagging: Unit-Varianten als bloßer String (`"AutoExec"`),
// Varianten mit Feldern als Objekt mit dem Variantennamen als einzigem
// Schlüssel (`{"Confirm": {"reason": "..."}}`). Feldnamen bleiben
// snake_case (`new_content`, nicht `newContent`). Die eigenen
// Event-Payloads in `crate::events` tragen dagegen `rename_all =
// "camelCase"`.

export type NoteTarget = { Server: string } | { Group: string };

/** Gemeinsame Form für `HostKeyDialog` — sowohl
 * `HostKeyVerificationNeededEvent` (Spec 0007, regulärer `connect()`) als
 * auch `TestConnectionResult`s `hostKeyUnknown`/`hostKeyMismatch` (Spec
 * 0008, `test_connection`) erfüllen diese Form strukturell, sodass sich
 * derselbe Dialog für beide Flüsse wiederverwenden lässt (Spec 0008,
 * Abschnitt 7: "denselben Bestätigungs-/Warnungs-Dialog wiederverwenden").
 */
export interface HostKeyInfo {
  host: string;
  port: number;
  kind: HostKeyKind;
  fingerprint: string;
  /** Nur bei `kind === "mismatch"` gesetzt. */
  expectedFingerprint: string | null;
  /** Algorithm of the key the server offers now (e.g. `ssh-ed25519`);
   * `null` when it could not be determined. Display-only. */
  keyType?: string | null;
}

/** Issue #696: one rule behind an automatic run; `segmentIndices` are the
 * top-level segments of a chain it covered. */
export interface AutoExecRule {
  ruleId: string;
  patternType: PatternType;
  patternValue: string;
  scope: Scope;
  segmentIndices: number[];
}

// --- Spec 0008: Server-/Gruppen-Verwaltung ------------------------------

/** Issue #100: warum ein gespeicherter Server nicht nutzbar ist —
 * `unknown_auth_method`: von einer neueren Version gespeichert, diese kennt
 * die Anmeldeart nicht; `unreadable_auth_method`: gespeicherte Anmeldeart
 * beschädigt. */
export type UnusableReason = "unknown_auth_method" | "unreadable_auth_method";

export type ConnectStepStatus =
  | { state: "running" }
  | { state: "ok" }
  | { state: "skipped" }
  | { state: "warning"; code: string }
  | { state: "failed"; code: string };

/** Spec 0050, Teil 3 — s. `crate::commands::TestAiProviderCredentialsResult`
 * Doc-Kommentar für die Mapping-Entscheidung (`RateLimited`/
 * `ProviderUnavailable`/etc. fallen alle unter `unreachable`, nicht nur
 * echte Netzwerkfehler). */
export type TestAiProviderCredentialsResult =
  | { kind: "valid" }
  | { kind: "authenticationFailed" }
  | {
      kind: "unreachable";
      message: string;
      /** Spec 0069, Teil A4/E3 (BL-0153): additiv, optional — `null`/
       * `undefined` fällt im Frontend auf `message` zurück. */
      code?: string | null;
    };

// --- Spec 0009: Filter-Regel-Verwaltung ---------------------------------
//
// `Scope`/`RuleAction` (core::filter) tragen wie `Decision` oben kein
// `serde(rename_all)` — Standard-Außen-Tagging, s. Modul-Kommentar am
// Dateianfang. `PatternType` (app-shell-DTO) hat dagegen
// `#[serde(rename_all = "snake_case")]`, also lowercase-Strings.

// --- Spec 0011: Regel-Schnellvorschlag im Bestätigungsdialog -----------

// --- Issue #596: explanation of a pending confirmation -------------------

// --- Spec 0012: KI-generierte Dokumente ---------------------------------

// --- Issue #545: KI-Rückfragen mit Antwortoptionen ------------------------

/** One answer option of an AI question. */
export interface QuestionOption {
  label: string;
  description: string | null;
}

// --- Spec 0017: Multi-Tab-Sessions --------------------------------------

/** Spec 0104: Kennzeichnung einer MCP-Sitzung (eigener Tab je Server und
 * MCP-Client). */
export interface McpSessionInfo {
  /** `clientInfo.name` aus dem MCP-Handshake, `null` ohne Namen. */
  clientName: string | null;
}

/** Spec 0034, Abschnitt 6/8: bereits geladene Historie eines Tabs (leer bei
 * `connect()`, befüllt nach `resume_chat_session()`), s.
 * `get_chat_history`. */
export type ChatHistoryRole = "user" | "assistant" | "action_result";

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser ---------------------

/** Issue #128 / Spec 0054, Teil 3: Ordner-Upload. */
export type FolderSkipReason =
  | "symlink"
  | "tooDeep"
  | "unsupported"
  | "notGranted"
  | "overwriteNotConfirmed";

export interface FolderSkippedEntryDto {
  path: string;
  reason: FolderSkipReason;
}

export interface FolderFailedEntryDto {
  path: string;
  error: string;
}

export interface FolderUploadPreviewDto {
  folderName: string;
  fileCount: number;
  folderCount: number;
  /** Remote paths of existing files the upload would overwrite. */
  overwrites: string[];
  skipped: FolderSkippedEntryDto[];
}

export interface FolderUploadSummaryDto {
  folderName: string;
  filesUploaded: number;
  foldersCreated: number;
  skipped: FolderSkippedEntryDto[];
  failed: FolderFailedEntryDto[];
  notAttempted: number;
}

// --- Spec 0028: MCP-Server-Integration -----------------------------------

export interface McpServerSettingsDto {
  enabled: boolean;
  /** `http://127.0.0.1:<port>` — der Wert für die externe Client-Konfiguration. */
  endpoint: string;
  token: string;
  allowedServerIds: string[];
  /** Spec 0028, Abschnitt 7 — wirkt erst auf den nächsten Serverstart. */
  confirmTimeoutSecs: number;
}

// --- Spec 0052: Versions- & Build-Anzeige ---------------------------------

// --- Spec 0071: Systemschlüsselbund-Zustand -------------------------------

/** Warum der Systemschlüsselbund nicht verfügbar ist — von
 * `crate::dto::KeychainUnavailableReasonDto` (Spec 0071, A1). */
export type KeychainUnavailableReason =
  | "no_session_bus"
  | "no_secret_service_provider"
  | "locked"
  | "unknown";

/** Von `app_logic::data_paths::DataPathEntryDto` (Issue #16): ein
 * wirksamer Datenpfad der laufenden Instanz. Nur der Pfad, nie Inhalt. */
export interface DataPathEntryDto {
  /** Stabiler Bezeichner (`database`, `logs`, `hostKeys`, `wrappingFile`,
   * `mcpSettings` oder ein Eintrag einer Edition). */
  id: string;
  /** `null` für eingebaute Einträge (Beschriftung aus dem
   * Übersetzungskatalog), sonst Anzeigetext der Edition. */
  label: string | null;
  path: string;
  isDirectory: boolean;
}

// --- Spec 0076: Anmeldung mit einer Schlüsseldatei -----------------------

// --- Spec 0075: Import/Export von OpenSSH-`ssh_config` -------------------

/** Von `crate::ssh_config_apply::OriginDto` — woher ein Feldwert stammt
 * (§3.1.3): welche Datei, welche Zeile, welcher `Host`-Block. */
export interface SshConfigOriginDto {
  file: string;
  line: number;
  block: string;
}

/** Ein Feldwert mit seiner Herkunft (`crate::ssh_config_apply::
 * SourcedDto`). `origin: null` heißt: Vorgabe des Produkts, nicht aus der
 * Datei (§3.1.2, z. B. `port` ohne `Port`-Zeile). */
export interface SshConfigSourcedDto<T> {
  value: T;
  origin: SshConfigOriginDto | null;
}

/** §5.2a verlangt, „die betroffene Regel" zu nennen — `action` ist das
 * sicherheitsrelevante Feld: Nur eine `allow`-Regel kann ein importiertes
 * Profil von `Confirm` auf `Allow` heben, eine `deny`-Regel bleibt ohnehin
 * wirksam. */
export interface SshConfigPreviewMatchedRuleDto {
  ruleId: string;
  action: "allow" | "confirm" | "deny";
}

export interface SshConfigPreviewTagDto {
  tag: string;
  origin: SshConfigOriginDto;
  /** §5.2a: nicht leer ⇒ dieses Schlagwort trifft diese bestehenden
   * Filterregeln. */
  matchedRules: SshConfigPreviewMatchedRuleDto[];
  /** `true` ⇒ das Schlagwort trägt **keinen** Platzhalter (buchstäbliche
   * Angabe in einem gemischten `Host`-Block, z. B. `Host prod *`) und
   * trifft eine `Scope::Tag`-Regel **exakt** — deutlicher zu kennzeichnen
   * als ein Muster-Schlagwort (§5.2a). Nur Kennzeichnung — die Vorgabe
   * trägt `defaultSelected`. */
  isLiteral: boolean;
  /** §5.2a/§9: `false` ⇒ das Schlagwort startet in der Vorschau abgewählt
   * (trifft eine Tag-Allow-Regel und keine Tag-Deny-Regel — gleich, ob
   * buchstäblich oder Muster). Vom Backend berechnet; der Dialog übernimmt
   * den Wert unverändert, und `apply` ohne Wahl folgt derselben Vorgabe. */
  defaultSelected: boolean;
}

export interface SshConfigPreviewIdentityFileDto {
  path: string;
  origin: SshConfigOriginDto;
  /** §3.1.9 (a): `false` ⇒ beim Verbinden nicht benutzbar (relativer Pfad
   * oder `~user/…`-Form), absoluter Pfad nötig. */
  usableWhenConnecting: boolean;
}

/** Wohin ein `jump_host` zeigt — `name` ist immer der tatsächliche Name
 * (Bestand: `Server.name`; geplant: der Alias in diesem Import),
 * `existing` unterscheidet beide Fälle. */
export interface SshConfigPreviewJumpDto {
  name: string;
  existing: boolean;
}

export interface SshConfigPreviewConflictDto {
  kind: "name" | "address";
  existingName: string;
}

export interface SshConfigPreviewEntryDto {
  index: number;
  name: string;
  group: number;
  host: SshConfigSourcedDto<string>;
  port: SshConfigSourcedDto<number>;
  username: SshConfigSourcedDto<string>;
  tags: SshConfigPreviewTagDto[];
  identityFile: SshConfigPreviewIdentityFileDto | null;
  jumpHost: SshConfigPreviewJumpDto | null;
  conflict: SshConfigPreviewConflictDto | null;
}

export interface SshConfigPreviewGroupDto {
  name: string;
  /** Index der Elterngruppe in `ImportPreviewDto.groups`, `null` = Wurzel. */
  parent: number | null;
  sourcePath: string;
}

export interface SshConfigPreviewFileDto {
  path: string;
  depth: number;
  status: "read" | "notSshConfig" | "unreadable" | "alreadyRead";
}

/** Warum eine Zeile nicht übernommen wurde (§3.1.5) — der Text dazu kommt
 * aus dem Übersetzungskatalog (`sshConfigImport.skipReason.*`), nie aus
 * dem Backend, s. `crate::ssh_config_apply::reason_key`. */
export type SshConfigSkipReasonKey =
  | "unsupported"
  | "alreadySet"
  | "emptyValue"
  | "outsideHostBlock"
  | "invalidValue"
  | "emptyAlias"
  | "mixedWildcardBlock"
  | "includeTooDeep"
  | "includeAlreadyRead"
  | "includeUnreadable"
  | "includeNotSshConfig"
  | "includeNoMatch"
  | "proxyJumpCycle"
  | "proxyJumpHopUnresolved"
  | "proxyJumpIntermediateExists"
  | "proxyJumpAmbiguousPredecessor"
  | "proxyJumpOwnAndIntermediate"
  | "proxyJumpLocalPseudoServer";

export interface SshConfigPreviewSkippedDto {
  file: string;
  line: number;
  /** Der **Name** der Direktive — `null` für „unlesbare Zeile" (§3.1.5).
   * Nie ein Wert. */
  directive: string | null;
  reason: SshConfigSkipReasonKey;
  entry: string | null;
}

/** Von `crate::ssh_config_apply::ImportPreviewDto` (§3.1.7) — die
 * vollständige Vorschau, bevor irgendetwas angelegt wird. */
export interface SshConfigImportPreviewDto {
  groups: SshConfigPreviewGroupDto[];
  entries: SshConfigPreviewEntryDto[];
  files: SshConfigPreviewFileDto[];
  skipped: SshConfigPreviewSkippedDto[];
  /** §3.1.9 (b): welche Dateien auf Weg (b) geöffnet **würden** — genannt,
   * bevor irgendetwas passiert. */
  identityFilePaths: string[];
}

/** Was mit `IdentityFile` geschehen soll (§3.1.9); Vorgabe `keepAsFile`
 * (a) — der einzige Weg, der keine Datei öffnet. */
export type SshConfigIdentityMode = "keepAsFile" | "intoKeychain" | "drop";

/** Die Wahl zu **einem** Eintrag der Vorschau — Eingabe für
 * `applySshConfigImport` (`crate::ssh_config_apply::EntryChoice`). */
export interface SshConfigEntryChoice {
  index: number;
  selected: boolean;
  identityMode: SshConfigIdentityMode;
  renameTo: string | null;
  droppedTags: string[];
}

export type SshConfigIdentityFallbackReason =
  | "missing"
  | "unreadable"
  | "notAKey"
  | "notARegularFile"
  | "tooLarge"
  | "pathNotAbsolute";

export interface SshConfigIdentityFallbackDto {
  entry: string;
  path: string;
  reason: SshConfigIdentityFallbackReason;
}

/** §3.1.9 (b), letzter Punkt: ein auf Weg (b) übernommener Schlüssel war
 * verschlüsselt — die Vorschau konnte das nicht wissen (§5.1), deshalb
 * steht es erst hier. Die Passphrase muss nachgetragen werden, bevor die
 * erste Verbindung gelingt. */
export interface SshConfigIdentityEncryptedDto {
  entry: string;
  path: string;
}

/** Von `crate::ssh_config_apply::ApplyOutcome` — Grundlage der
 * Abschlussmeldung nach dem Bestätigen. */
export interface SshConfigApplyOutcomeDto {
  createdServers: number;
  createdGroups: number;
  skippedConflicts: number;
  identityFallbacks: SshConfigIdentityFallbackDto[];
  identityEncrypted: SshConfigIdentityEncryptedDto[];
}

/** Von `crate::ssh_config_export::ExportedServerDto` (§3.2.7). */
export interface SshConfigExportedServerDto {
  originalName: string;
  alias: string;
  renamed: boolean;
}

/** Von `crate::ssh_config_export::ExportResultDto`. */
export interface SshConfigExportResultDto {
  path: string;
  exported: SshConfigExportedServerDto[];
  /** §3.2.5: fertig formatierte `Include <pfad>`-Zeile. */
  includeHint: string;
}

// --- Spec 0101, Etappe 3: Master-Passwort und Entsperrmaske --------------

/** Welche Maske der Start zeigt (`crate::commands::master_password::
 * StartupScreen`). **Ein** Feld statt mehrerer Flaggen — die vier Zustände
 * der Verpackungsdatei aus A3 schließen sich gegenseitig aus, und die
 * Oberfläche soll sie nicht aus Wahrheitswerten zusammensetzen müssen. */
export type StartupScreen =
  | "unlocked"
  | "unlock"
  | "unusableWrapping"
  | "unreachableWrapping"
  | "setUpMasterPassword";

/** Von `crate::commands::master_password::StartupStateDto` (A16/A18,
 * Klarstellung 11). Trägt bewusst **kein** Passwort und keinen Schlüssel
 * (A19/§6). */
export interface StartupStateDto {
  screen: StartupScreen;
  /** A18: der aktive Modus. */
  mode: "password" | "keychain";
  /** Klarstellung 11: Darf die Maske „Neu anfangen" anbieten? Die
   * Oberfläche entscheidet das **nicht** selbst — sie fragt, und das
   * Kommando prüft dieselbe Bedingung noch einmal. */
  offersStartOver: boolean;
  /** Klarstellung 11: gescheiterte Entsperrversuche dieses Programmlaufs,
   * für den Hinweistext. */
  failedUnlockAttempts: number;
  /** ADR 0095 §8: Vor der Entsperrung ist die gespeicherte Sprachwahl aus
   * Spec 0024 nicht lesbar (das `store`-Plugin ist noch nicht
   * registriert). Die Startmasken nehmen deshalb die Sprache, die das
   * Backend einmal aus der Umgebung bestimmt hat (Spec 0071, A11a/A11b). */
  language: "de" | "en";
}

/** Welche Knöpfe eine Startfrage anbietet (`crate::window_prompt::
 * PromptKind`). Das Frontend bildet sie ab, es erfindet keine — jeder
 * Knopf, der hier nicht steht, darf dort nicht erscheinen. */
export type StartupPromptKind =
  | "retryOrQuit"
  | "retrySkipOrQuit"
  | "retrySetUpOrQuit"
  | "startOverOrQuit"
  | "newKeyOrQuit"
  | "confirmStartOver"
  | "confirmNewKey"
  | "newMasterPassword"
  | "notice";

/** Von `crate::window_prompt::StartupPromptRequest`. Titel und Text kommen
 * **fertig übersetzt** aus dem Backend (`app_logic::
 * startup_choice_dialogs`, Sprache aus [`StartupStateDto.language`]) — nur
 * die Knopfbeschriftungen stehen in den Locale-Dateien. */
export interface StartupPromptRequest {
  kind: StartupPromptKind;
  title: string;
  message: string;
}

/** Von `crate::window_prompt::StartupPromptAnswer`. */
export type StartupPromptAnswer =
  | "retry"
  | "quit"
  | "startOver"
  | "generateNewKey"
  | "continueWithoutMigration"
  | "setUpMasterPassword"
  | "confirm"
  | "cancel";

/** Spec 0102: Ergebnis von `sftp_start_directory` — Startpfad des
 * Dateibrowsers (`"."` = Home) und, genau einmal pro Sitzung, der
 * konfigurierte Wert, falls das Startverzeichnis fehlt. */
export interface StartDirectoryDto {
  path: string;
  missingDirectory: string | null;
}

/** Spec 0102: Ergebnis von `open_terminal`. */
export interface TerminalStartDto {
  missingDirectory: string | null;
}
