// IPC types come from the Rust definitions: `generated/ipc.ts` is written by
// `npm run gen:ipc` (issue #919). This file re-exports them under their
// existing names and keeps only the hand-written helpers and the types that do
// not cross the IPC boundary as a DTO.

import type {
  ActionOrigin,
  ConnectionStatus,
  Decision,
  PatternType,
  ProviderType,
  Scope,
  WebActivityDto,
} from "./generated/ipc";

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
} from "./generated/ipc";

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

/** Spec 0016, Abschnitt 6 — löst das frühere `{ target_type, target_id }`
 * ab: die KI wählt nur noch relativ zur aktuellen Session, nie eine
 * konkrete `ServerId`/`GroupId` (die löst das Backend selbst auf). Als
 * reiner Unit-Varianten-Enum serialisiert Rusts Serde-Default-Tagging das
 * als bloßen String (`"CurrentServer"`/`"CurrentServerGroup"`), nicht als
 * Objekt wie `NoteTarget`. */
export type NoteTargetSelector = "CurrentServer" | "CurrentServerGroup";

export type AiAction =
  | { SuggestCommand: { command: string } }
  | { ProposeNoteUpdate: { target: NoteTargetSelector; new_content: string } }
  | { GenerateDocument: { title: string; content_markdown: string } }
  | { ReadRemoteFile: { path: string } }
  | { WriteRemoteFile: { path: string; content: string } };

/** Spec 0026, Abschnitt 2 — kein "grün": Abwesenheit eines Badges im UI
 * bedeutet bereits "laut bekannten Mustern unauffällig", kein
 * Sicherheitsversprechen. */
export type RiskLevel = "none" | "yellow" | "red";

export interface RiskAssessment {
  serverRisk: RiskLevel;
  serverRiskReason: string | null;
  dataRisk: RiskLevel;
  dataRiskReason: string | null;
  /** Spec 0026, Abschnitt 3: `true`, sobald die optionale KI-Zweitmeinung
   * tatsächlich eingeflossen ist. */
  aiReviewed: boolean;
}

/** Issue #611: the persisted chat session a runtime session uses now
 * (`null`: local pseudo-server / no-persist). */
export interface ChatSessionChangedEvent {
  sessionId: string;
  chatSessionId: string | null;
}

export interface ConnectionStatusChangedEvent {
  sessionId: string;
  status: ConnectionStatus;
  reason: string | null;
}

/** Issue #356, Spec 0107 §8: Stand der automatischen Neuverbindung. */
export interface AutoReconnectChangedEvent {
  sessionId: string;
  phase: "running" | "reconnected" | "gave_up" | "stopped" | "blocked";
  /** Nummer des laufenden bzw. nächsten Versuchs (nur bei `running`). */
  attempt: number;
}

export type HostKeyKind = "unknown" | "mismatch";

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

export interface HostKeyVerificationNeededEvent extends HostKeyInfo {
  sessionId: string;
  /** Issue #37: Kennung genau dieser Abfrage. Ein `connect()`-Retry kann
   * unter derselben `sessionId` eine neue Abfrage anlegen; das passende
   * `HostKeyVerificationEndedEvent` trägt dieselbe `promptId`. */
  promptId: number;
}

/** Issue #37: warum das Backend nicht mehr auf eine Host-Key-Abfrage wartet. */
export type HostKeyPromptEndReason = "decided" | "timed_out" | "abandoned";

/** Issue #37: `host-key-verification-ended` — das Backend wartet nicht mehr
 * auf die Abfrage (`sessionId`, `promptId`). Rein informativ: Timeout und
 * Abbruch gelten im Backend bereits als Ablehnung. */
export interface HostKeyVerificationEndedEvent {
  sessionId: string;
  promptId: number;
  reason: HostKeyPromptEndReason;
}

/** Issue #546: detail of a connect step — a stable code plus parameters, to
 * be translated by the frontend. Never carries secrets. */
export type ConnectStepDetail =
  | { code: "contacting"; target: string }
  | { code: "tunnel"; hop: string }
  | { code: "waitingForResponse" }
  | { code: "waitingForDecision" }
  | { code: "authMethod"; method: string };

/** Issue #546: one step in a `connect-progress` event. */
export interface ConnectStepSnapshot {
  kind:
    | "dnsResolution"
    | "tcpConnect"
    | "tunnelOpen"
    | "handshake"
    | "hostKeyCheck"
    | "authentication"
    | "sessionReady";
  hopIndex: number;
  /** `user@host:port` of the hop (Spec 0076 naming). */
  hop: string;
  status: "running" | "ok" | "skipped" | "failed";
  errorCode: string | null;
  detail: ConnectStepDetail | null;
}

/** Issue #546: `connect-progress` — the full current step list of a running
 * connect. `attempt` is 1-based and counts retries (e.g. after Trust); a new
 * attempt starts a fresh list. */
export interface ConnectProgressEvent {
  attemptId: string;
  serverId: string;
  attempt: number;
  steps: ConnectStepSnapshot[];
}

export interface TerminalOutputEvent {
  sessionId: string;
  /** Base64-kodiert (s. `crate::events`-Modul-Kommentar). */
  data: string;
}

export interface ChatTextDeltaEvent {
  sessionId: string;
  delta: string;
}

export interface ChatActionProposedEvent {
  sessionId: string;
  actionId: string;
  action: AiAction;
  decision: Decision;
  /** Spec 0019, Abschnitt 3: nur bei `ProposeNoteUpdate` gesetzt — aktueller
   * Inhalt des Ziels, für die Diff-Vorschau. */
  previousNoteContent: string | null;
  /** Spec 0018, Abschnitt 7: ob beim Ausführen automatisch ein hinterlegtes
   * Sudo-Passwort eingespeist würde. */
  usesStoredSudoPassword: boolean;
  /** Spec 0020, Abschnitt 4.2, Punkt 3: nur bei `WriteRemoteFile` gesetzt —
   * aktueller Inhalt der Zieldatei für die Diff-Vorschau (dieselbe
   * `NoteDiffPreview`-Komponente wie bei `ProposeNoteUpdate`). `null`, wenn
   * die Datei noch nicht existiert ODER sich nicht als Text dekodieren
   * lässt (dann ist stattdessen `previousFileSize` gesetzt). */
  previousFileContent: string | null;
  /** Nur gesetzt, wenn `previousFileContent` wegen einer Binärdatei `null`
   * ist — Größe der ALTEN Datei in Bytes, für einen Größenvergleich-Hinweis
   * statt einer Diff-Ansicht. */
  previousFileSize: number | null;
  /** Spec 0023, Abschnitt 3: nur bei `action: ProposeNoteUpdate` gesetzt —
   * Server- oder Gruppenname des bereits serverseitig aufgelösten Ziels.
   * Immer anzeigen, auch wenn es der aktuell offene Server der Session ist
   * (Konsistenz statt Redundanzvermeidung) — genau das fehlende Stück, das
   * den in Spec 0023 gemeldeten Bug verursacht hat. `null` nur, wenn die
   * Zielauflösung serverseitig fehlschlägt (z. B. Server inzwischen
   * gelöscht). */
  targetName: string | null;
  /** Spec 0026, Abschnitt 2: nur für `SuggestCommand`/`ReadRemoteFile`/
   * `WriteRemoteFile` gesetzt (`null` für `ProposeNoteUpdate`) — bereits
   * die regelbasierte Einschätzung, `aiReviewed: false` beim ersten Event.
   * Kann später über `risk-assessment-updated` (s. `events.ts`) auf der
   * Daten-Risiko-Achse angehoben werden. */
  riskAssessment: RiskAssessment | null;
  /** Spec 0028, Abschnitt 6/9a. */
  origin: ActionOrigin;
  /** Issue #696: only for `decision: "AutoExec"` — the rules that let the
   * command run without confirmation (display only). Empty otherwise. */
  autoExecRules?: AutoExecRule[];
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

/** Spec 0026, Abschnitt 3, Punkt 4. */
export interface RiskAssessmentUpdatedEvent {
  sessionId: string;
  actionId: string;
  dataRisk: RiskLevel;
  reason: string | null;
  /** Issue #102: die Zweitmeinung konnte nicht eingeholt werden
   * (Provider-Fehler oder kein erkennbares Urteil). `dataRisk` ist dann die
   * regelbasierte Stufe. Optional, damit ältere Payloads weiter passen. */
  secondOpinionUnavailable?: boolean;
}

/** Spec 0092, A3.2: Die Entscheidung zu `actionId` hat sich NACH
 * `chat-action-proposed` von `AutoExec` zu `Confirm` verschärft — die
 * KI-Zweitmeinung hat das Daten-Risiko auf Rot gehoben. Kommt immer NACH
 * `RiskAssessmentUpdatedEvent` für dieselbe `actionId` (s. `events.ts`).
 * `reason`/`code` bauen sich die Karte und die Tab-Leiste selbst zu einem
 * `Decision.Confirm` zusammen, weil dieses Event (anders als
 * `chat-action-proposed`) nicht den ganzen `Decision`-Enum überträgt. */
export interface ActionDecisionEscalatedEvent {
  sessionId: string;
  actionId: string;
  reason: string;
  code: string;
}

/** Spec 0010 — `action` ist hier immer `{ ProposeNoteUpdate: {...} }`, nie
 * `SuggestCommand` (s. `crate::events::NoteUpdateSuggestedPayload`). Kein
 * `decision`-Feld (anders als `ChatActionProposedEvent`): wäre für
 * `ProposeNoteUpdate` ohnehin immer "Confirm". */
export interface NoteUpdateSuggestedEvent {
  sessionId: string;
  actionId: string;
  action: { ProposeNoteUpdate: { target: NoteTargetSelector; new_content: string } };
  /** Spec 0019, Abschnitt 3 — s. `ChatActionProposedEvent`-Doc-Kommentar. */
  previousNoteContent: string | null;
  /** Spec 0023, Abschnitt 3 — s. `ChatActionProposedEvent.targetName`-Doc-
   * Kommentar. Besonders wichtig für dieses Event: es ist bewusst app-weit
   * statt tab-gebunden (Spec 0010, Abschnitt 2, Punkt 6), der Nutzer hat
   * beim Empfang womöglich einen ganz anderen Server offen. */
  targetName: string | null;
  /** Spec 0057, §4.2 (Issue #94): `true` nur, wenn der Vorschlag aus "Ja,
   * zusammenfassen" stammt und die KI-Antwort vom Provider wegen des
   * Längenlimits abgeschnitten wurde. Jeder andere Vorschlag: `false`. */
  summaryIncomplete: boolean;
}

/** Spec 0057, §4.2 (Etappe 4) — der ERSTE Dialog ("Deine Notiz für diesen
 * Server ist sehr groß …"), bevor überhaupt ein KI-Aufruf stattgefunden hat.
 * Bewusst kein `sessionId` (s. `crate::events::NoteShrinkSuggestedPayload`):
 * der Vorschlag bezieht sich immer auf den SERVER, nie auf eine bestimmte
 * Sitzung. */
export interface NoteShrinkSuggestedEvent {
  serverId: string;
  serverName: string;
}

/** Spec 0057, §4.2/§6: Gegenstück zu einem erfolgreichen `note-update-
 * suggested` nach "Ja, zusammenfassen" — der KI-Aufruf ist fehlgeschlagen
 * oder abgelaufen, die gespeicherte Notiz bleibt unverändert. */
export interface NoteShrinkFailedEvent {
  serverId: string;
  message: string;
}

/** Spec 0058 (Politur-Paket), spec-reviewer-Fund: das Erfolgs-Gegenstück zu
 * `NoteShrinkFailedEvent` — ein zeitgleich offener Notiz-Editor (z. B. über
 * den neuen "Jetzt zusammenfassen"-Link im Editor selbst ausgelöst, s.
 * `NotesPanel.tsx`) muss nach Zustimmung neu laden, sonst überschreibt sein
 * nächster "Speichern"-Klick die gerade akzeptierte Zusammenfassung. */
export interface NoteShrinkSucceededEvent {
  serverId: string;
}

/** Spec 0021, Abschnitt 5: signalisiert eine *automatische* Folgerunde (die
 * KI antwortet auf ein Aktionsergebnis, ohne dass der Nutzer getippt hat) —
 * Grundlage für den "Automatik läuft"-Indikator. `round` ist nur zur
 * Diagnose/Anzeige gedacht (z. B. "Runde 3"), keine Ablauflogik hängt im
 * Frontend daran. */
export interface ChatAutoContinuationStartedEvent {
  sessionId: string;
  round: number;
}

/** Spec 0061, Abschnitt 4: die App wartet proaktiv vor einem KI-Aufruf,
 * weil das aus den Rate-Limit-Headern bekannte Restbudget knapp ist (oder
 * der geschätzte Request es überschreiten würde) — rein informativ, kein
 * Abbrechen-/Trotzdem-Button, der Request geht nach `waitSeconds`
 * automatisch raus. */
export interface AiBudgetWaitingEvent {
  sessionId: string;
  waitSeconds: number;
}

export type ActionResultPayload =
  | {
      kind: "command";
      command: string;
      stdout: string;
      stderr: string;
      exitCode: number | null;
      /** Spec 0027: `true`, wenn der Nutzer dieses Kommando manuell
       * abgebrochen hat, bevor es von selbst beendet war — dann ist
       * `exitCode` immer `null`, keine Störung. */
      cancelled: boolean;
      /** Spec 0043, Fund A: `true`, wenn `stdout`/`stderr` beim Streaming am
       * konfigurierten Output-Cap abgeschnitten wurden — Ausgabe ist
       * unvollständig, nicht fehlerhaft. */
      truncated: boolean;
    }
  | { kind: "noteUpdate"; summary: string }
  /** Spec 0020, Abschnitt 4.1 — `content` ist bereits redigiert (Spec 0006). */
  | { kind: "fileRead"; path: string; content: string }
  /** Spec 0020, Abschnitt 4.2/4.3 — `backupPath` ist `null` nur, wenn die
   * Datei vor dem Schreiben nicht existierte. */
  | { kind: "fileWrite"; path: string; backupPath: string | null; usedSudoPassword: boolean };

/** Issue #693 (Spec 0039, §5.2): state of the injection check of one
 * checked item. `reason` is untrusted text — render as plain text only. */
export type InjectionCheckState =
  | { state: "running" }
  | { state: "clean" }
  | { state: "suspected"; reason: string }
  | { state: "unavailable" };

/** Issue #693 — `chat-injection-check`; `itemId` is the action id of a
 * command result / file read, or the activity id of a web research card. */
export type ChatInjectionCheckEvent = { sessionId: string; itemId: string } & InjectionCheckState;

export interface ChatActionResultEvent {
  sessionId: string;
  actionId: string;
  result: ActionResultPayload;
}

/** Issue #325 (Spec 0106): live output of a running `SuggestCommand`.
 * `stdout`/`stderr` are newly released, already redacted text to append;
 * at most about ten events per second and action. Display only — the
 * final `chat-action-result` replaces it. */
export interface ChatActionOutputEvent {
  sessionId: string;
  actionId: string;
  stdout: string;
  stderr: string;
  /** `true` once the output size limit was reached. */
  truncated: boolean;
}

/** Issue #327 — the backend's guess whether a running command waits for
 * input; `waiting: false` when output arrived again. Display only. */
export interface ChatActionWaitingInputEvent {
  sessionId: string;
  actionId: string;
  waiting: boolean;
}

export interface ChatErrorEvent {
  sessionId: string;
  message: string;
  /** Spec 0024, Abschnitt 5 — stabiler Backend-Code (`SshError`/`AiError`),
   * `null`/`undefined` wenn keiner vorliegt (reine Validierungsmeldung ohne
   * typisierten Fehler dahinter). Übersetzung via `translateErrorCode`. */
  code?: string | null;
  /** Issue #600 — provider details for the "Details" view (the message is
   * already redacted by the backend). */
  providerErrorType?: string | null;
  httpStatus?: number | null;
  providerMessage?: string | null;
  /** Issue #694 — set for the timeout errors: provider name and the exceeded
   * limit in seconds. */
  providerName?: string | null;
  timeoutSecs?: number | null;
  /** Issue #631 — when access returns after a provider usage limit. */
  accessReturns?: string | null;
}

/** Issue #694 — a request to the AI provider was sent; the chat shows the
 * wait line until content arrives. */
export interface ChatAiRequestStartedEvent {
  sessionId: string;
  providerName: string;
}

/** Issue #600 — the AI provider is overloaded; the request is repeated
 * automatically after `delaySecs`. */
export interface ChatAiRetryingEvent {
  sessionId: string;
  attempt: number;
  maxAttempts: number;
  delaySecs: number;
}

export interface ChatAutoContinuationLimitReachedEvent {
  sessionId: string;
  limit: number;
}

/** Spec 0065, Teil 2 — die zuletzt gestreamte KI-Antwort wurde durch das
 * Längenlimit abgeschnitten (kein Tool-Call betroffen, der Fall läuft
 * separat über den Retry). Zeigt einen Hinweis + „Weiter"-Aktion
 * (`continue_truncated_response`) an der letzten KI-Nachricht. */
export interface ChatResponseTruncatedEvent {
  sessionId: string;
}

/** Issue #601: real input size of the last request vs. the model window. */
export interface ChatContextUsageEvent {
  sessionId: string;
  inputTokens: number;
  windowTokens: number;
}

/** Issue #601: older chat parts were summarized/cut for a request. */
export interface ChatContextCompactedEvent {
  sessionId: string;
}

/** Spec 0105 — `chat-web-activity`. */
export interface ChatWebActivityEvent {
  sessionId: string;
  /** Issue #693: binds `chat-injection-check` events to this card. */
  activityId?: string;
  activity: WebActivityDto;
}

/** Spec 0080, A2 — eine Runde endete ohne jeden Text und ohne vorgeschlagene
 * Aktion (z. B. ein Reasoning-Modell, dessen Denk-Tokens das ganze Budget
 * vor jedem Text aufbrauchen). Zeigt einen reinen Hinweis ohne „Weiter" und
 * ohne Export-/Notiz-Leiste — es gibt nichts, das fortgesetzt oder
 * exportiert werden könnte. */
export interface ChatResponseEmptyEvent {
  sessionId: string;
}

/** Issue #647 — the model declined the request (`refusal` /
 * `content_filter`). Shows a plain notice; text streamed before stays. */
export interface ChatResponseRefusedEvent {
  sessionId: string;
}

/** Spec 0105 §7 (Issue #169) — das Provider-Konto hat die Web-Werkzeuge
 * abgeschaltet; die Sitzung fragt ab jetzt ohne Web-Recherche weiter. Trägt
 * bewusst keinen Fehlertext des Providers. */
export interface ChatWebResearchUnavailableEvent {
  sessionId: string;
}

/** Spec 0066, §1 — der Nutzer hat die laufende KI-Anfrage per Stopp
 * abgebrochen. */
export interface ChatResponseCancelledEvent {
  sessionId: string;
}

/** Spec 0066, §2 — während eines laufenden Turns eingereihte Nachrichten
 * wurden jetzt an die KI übergeben. */
export interface ChatQueuedMessagesSentEvent {
  sessionId: string;
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

/** Spec 0012, Abschnitt 3 — kein `decision`-Feld: `GenerateDocument`
 * durchläuft nie einen Bestätigungsdialog (s. `crate::events`-Payload). */
export interface ChatDocumentGeneratedEvent {
  sessionId: string;
  actionId: string;
  title: string;
  contentMarkdown: string;
}

// --- Issue #545: KI-Rückfragen mit Antwortoptionen ------------------------

/** One answer option of an AI question. */
export interface QuestionOption {
  label: string;
  description: string | null;
}

/** Issue #545: payload of `chat-question-asked`. The question is never
 * executable; the answer is sent as a normal user message. */
export interface ChatQuestionAskedEvent {
  sessionId: string;
  questionId: string;
  question: string;
  options: QuestionOption[];
  multiSelect: boolean;
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

/** Issue #658: `delete` / `chmod` are recursive folder operations. */
export type SftpTransferKind = "upload" | "download" | "delete" | "chmod" | "chown";

export interface SftpTransferStartedEvent {
  sessionId: string;
  transferId: string;
  kind: SftpTransferKind;
  fileName: string;
  /** `null`, wenn die Größe vorab nicht ermittelbar war (s.
   * `crate::events`-Moduldoc zur Fortschritts-Design-Entscheidung: kein
   * echter Byte-Fortschritt, nur Start/Ende plus — falls bekannt —
   * Gesamtgröße). */
  totalBytes: number | null;
}

export interface SftpTransferFinishedEvent {
  sessionId: string;
  transferId: string;
  /** `null` bei Erfolg, sonst die Fehlermeldung. */
  error: string | null;
  /** Issue #622: stopped by the cancel button or by ending elevated rights
   * (no error). */
  stopped?: "cancelled" | "stopped" | null;
  /** Issue #622: a stopped upload whose partial remote file could not be
   * removed. */
  incompleteRemotePath?: string | null;
  /** Issue #658: recursive delete/chmod — entries changed before the end. */
  entriesDone?: number | null;
}

/** Issue #622: throttled progress of a running transfer. */
export interface SftpTransferProgressEvent {
  sessionId: string;
  transferId: string;
  bytesDone: number;
  totalBytes: number | null;
  /** Folder transfers: aggregate over all files. */
  folder?: {
    filesDone: number;
    filesTotal: number;
    bytesDone: number;
    bytesTotal: number;
  };
  /** Issue #658: recursive delete/chmod; `total` is `null` while counting. */
  entries?: { done: number; total: number | null };
}

// --- Spec 0028: MCP-Server-Integration -----------------------------------

/** Spec 0028, Abschnitt 9a / Spec 0104: signalisiert, für die MCP-Sitzung
 * `sessionId` einen Tab anzuzeigen (falls noch keiner existiert) — ohne zu
 * ihm zu wechseln (Spec 0104, §3). Nur für die vier aktionsauslösenden
 * MCP-Tools, nicht für `list_servers`/`get_server_notes`. */
export interface McpActionTabRequestedEvent {
  sessionId: string;
  serverId: string;
  /** `clientInfo.name` des MCP-Clients, `null` ohne Namen. */
  clientName: string | null;
}

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

/** Issue #628: a pending note confirmation timed out in the backend. */
export interface NoteUpdateExpiredEvent {
  actionId: string;
}
