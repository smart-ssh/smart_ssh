// Spiegelt die Rust-DTOs aus `crates/app-tauri/src/dto.rs` (Spec 0007,
// Abschnitt 8.2). Feldnamen camelCase, weil die DTO-Structs dort
// `#[serde(rename_all = "camelCase")]` tragen; `ProviderType`-Werte
// bleiben snake_case (exakt wie im SQL-`CHECK`-Constraint), weil dieser
// eine Typ stattdessen `#[serde(rename = "...")]` pro Variante nutzt.

export type ProviderType =
  | "openai"
  | "anthropic"
  | "generic_openai_compatible"
  | "ollama";

export type AuthMethodKind = "password" | "private_key" | "agent" | "certificate";

export interface ServerDto {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  groupId: string | null;
  tags: string[];
  authKind: AuthMethodKind;
  jumpHost: string | null;
  notes: string;
  /** Spec 0018, Abschnitt 4: ob ein Sudo-Passwort im Schlüsselbund
   * hinterlegt ist — nie der Wert selbst. */
  hasSudoPassword: boolean;
}

export interface AiProviderConfigDto {
  id: string;
  providerType: ProviderType;
  displayName: string;
  baseUrl: string | null;
  model: string;
  supportsNativeToolCalling: boolean;
  isActive: boolean;
  /** Spec 0025, Abschnitt 3. */
  extraHeaders: [string, string][];
  /** Spec 0025, Abschnitt 4. */
  attestationUrl: string | null;
}

export interface AiProviderConfigInput {
  providerType: ProviderType;
  displayName: string;
  baseUrl: string | null;
  model: string;
  supportsNativeToolCalling: boolean;
  apiKey: string;
  /** Spec 0025, Abschnitt 3. */
  extraHeaders: [string, string][];
  /** Spec 0025, Abschnitt 4. */
  attestationUrl: string | null;
}

export const PROVIDER_TYPE_LABELS: Record<ProviderType, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  generic_openai_compatible: "Generisch (OpenAI-kompatibel)",
  ollama: "Ollama",
};

// Nur bei diesen beiden Typen ist Base-URL relevant (Spec 0007, Abschnitt
// 8.3: "Base-URL-Feld nur bei generic_openai_compatible/ollama sichtbar").
export function needsBaseUrl(type: ProviderType): boolean {
  return type === "generic_openai_compatible" || type === "ollama";
}

// Spec 0025, Abschnitt 2: `discover_models` funktioniert nur gegen die
// OpenAI-kompatible Familie (`GET {base_url}/models`) — `anthropic` hat
// kein äquivalentes Endpoint-Verhalten.
export function supportsModelDiscovery(type: ProviderType): boolean {
  return type === "openai" || type === "generic_openai_compatible" || type === "ollama";
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

/** `code` (Spec 0024, Abschnitt 5): stabiler, sprachunabhängiger Bezeichner
 * der Grund-Art, fürs Mapping auf einen Übersetzungs-Key (s.
 * `errorCodes.ts`) — `reason` bleibt der bestehende (deutsche) Anzeigetext,
 * unverändert als Fallback, falls ein `code` nicht gemappt ist. */
export type Decision =
  | "AutoExec"
  | { Confirm: { reason: string; code: string } }
  | { Deny: { reason: string; code: string } };

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

/** Spec 0017, Abschnitt 2: `awaiting_host_key` kommt nie über
 * `connection-status-changed` (das Event kennt nur den Übergang
 * connected/disconnected), nur als `SessionSummaryDto.status`. */
export type ConnectionStatus = "connected" | "disconnected" | "awaiting_host_key";

export interface ConnectionStatusChangedEvent {
  sessionId: string;
  status: ConnectionStatus;
  reason: string | null;
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
}

export interface HostKeyVerificationNeededEvent extends HostKeyInfo {
  sessionId: string;
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
}

/** Spec 0026, Abschnitt 3, Punkt 4. */
export interface RiskAssessmentUpdatedEvent {
  sessionId: string;
  actionId: string;
  dataRisk: RiskLevel;
  reason: string | null;
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
    }
  | { kind: "noteUpdate"; summary: string }
  /** Spec 0020, Abschnitt 4.1 — `content` ist bereits redigiert (Spec 0006). */
  | { kind: "fileRead"; path: string; content: string }
  /** Spec 0020, Abschnitt 4.2/4.3 — `backupPath` ist `null` nur, wenn die
   * Datei vor dem Schreiben nicht existierte. */
  | { kind: "fileWrite"; path: string; backupPath: string | null; usedSudoPassword: boolean };

export interface ChatActionResultEvent {
  sessionId: string;
  actionId: string;
  result: ActionResultPayload;
}

export interface ChatErrorEvent {
  sessionId: string;
  message: string;
}

/** Antwort auf `host-key-verification-needed` (`confirm_host_key`). */
export type HostKeyUserDecision = { decision: "trust" } | { decision: "reject" };

/** Antwort auf ein `chat-action-proposed` mit `decision: Confirm` (`respond_to_action`). */
export type ActionUserDecision =
  | { decision: "approve" }
  | { decision: "deny" }
  | { decision: "editThenApprove"; command: string };

// --- Spec 0008: Server-/Gruppen-Verwaltung ------------------------------

export interface GroupDto {
  id: string;
  name: string;
  parentId: string | null;
  notes: string;
}

export interface DeleteGroupResult {
  childGroupsToDelete: GroupDto[];
  serversToUnassign: ServerDto[];
  executed: boolean;
}

/** Eingabe für `create_server`/`update_server`/`test_connection`. */
export interface ServerInput {
  name: string;
  host: string;
  port: number;
  username: string;
  groupId: string | null;
  tags: string[];
  auth: AuthMethodInput;
  jumpHost: string | null;
  /** Spec 0018, Abschnitt 4: leer/`null` = unverändert (bei `update`) bzw.
   * "kein Sudo-Passwort" (bei `create`). Entfernen eines bereits gesetzten
   * Werts läuft über `clearServerSudoPassword`, nicht über dieses Feld. */
  sudoPassword: string | null;
}

export type AuthMethodInput =
  | { kind: "password"; value: string | null }
  | { kind: "privateKey"; keyContent: string | null; passphrase: string | null }
  | { kind: "agent" }
  | { kind: "certificate"; certContent: string | null; keyContent: string | null };

export type NoteEditorDto =
  | { kind: "user" }
  | { kind: "ai"; provider: string; model: string };

export interface NoteRevisionDto {
  id: string;
  content: string;
  editedBy: NoteEditorDto;
  createdAt: string;
}

/** Spec 0008, Abschnitt 7 — s. `crate::dto::TestConnectionResult`-Doc-
 * Kommentar für die beiden Abweichungen von der Spec-Skizze
 * (`NetworkError` als Objekt, `host`/`port`/`rawKey` bei den
 * Host-Key-Varianten). */
export type TestConnectionResult =
  | { kind: "success" }
  | { kind: "authenticationFailed" }
  | { kind: "hostKeyUnknown"; host: string; port: number; rawKey: number[]; fingerprint: string }
  | {
      kind: "hostKeyMismatch";
      host: string;
      port: number;
      rawKey: number[];
      expectedFingerprint: string;
      actualFingerprint: string;
    }
  | { kind: "networkError"; message: string }
  | { kind: "timeout" };

// --- Spec 0009: Filter-Regel-Verwaltung ---------------------------------
//
// `Scope`/`RuleAction` (core::filter) tragen wie `Decision` oben kein
// `serde(rename_all)` — Standard-Außen-Tagging, s. Modul-Kommentar am
// Dateianfang. `PatternType` (app-tauri-DTO) hat dagegen
// `#[serde(rename_all = "snake_case")]`, also lowercase-Strings.

export type Scope = "Global" | { Server: string } | { Tag: string };

export type RuleAction = "Allow" | "Confirm" | "Deny";

export type PatternType = "glob" | "regex" | "exact";

export interface RuleDto {
  id: string;
  patternType: PatternType;
  patternValue: string;
  action: RuleAction;
  scope: Scope;
  priority: number;
}

/** Eingabe für `create_rule`/`update_rule`. */
export interface RuleInput {
  patternType: PatternType;
  patternValue: string;
  action: RuleAction;
  scope: Scope;
  priority: number;
}

/** Read-only-Anzeige eines Hard-Blacklist-Musters (`list_hard_blacklist`). */
export interface PatternDto {
  kind: PatternType;
  value: string;
}

/** Eingabe für `evaluate_explained` — `serverId: null` simuliert "kein
 * Server ausgewählt" (s. `crate::dto::EvalContextInput`-Doc-Kommentar). */
export interface EvalContextInput {
  serverId: string | null;
  tags: string[];
}

export interface EvaluationTraceDto {
  decision: Decision;
  matchedRule: string | null;
  matchedHardBlacklistEntry: string | null;
  subCommandTraces: EvaluationTraceDto[];
}

// --- Spec 0011: Regel-Schnellvorschlag im Bestätigungsdialog -----------

export interface PatternSuggestionDto {
  label: string;
  patternType: PatternType;
  patternValue: string;
}

// --- Spec 0012: KI-generierte Dokumente ---------------------------------

/** Spec 0012, Abschnitt 3 — kein `decision`-Feld: `GenerateDocument`
 * durchläuft nie einen Bestätigungsdialog (s. `crate::events`-Payload). */
export interface ChatDocumentGeneratedEvent {
  sessionId: string;
  actionId: string;
  title: string;
  contentMarkdown: string;
}

export type DocumentFormat = "markdown" | "word";

// --- Spec 0017: Multi-Tab-Sessions --------------------------------------

/** Sicht auf eine laufende (oder auf Host-Key-Bestätigung wartende) Session
 * für die Tab-Leiste — Grundlage für `list_sessions()`, dient dem
 * Wiederherstellen offener Tabs bei einem Frontend-Neuladen. */
export interface SessionSummaryDto {
  sessionId: string;
  serverId: string;
  serverName: string;
  status: ConnectionStatus;
  hasPendingAction: boolean;
}

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser ---------------------

export interface RemoteEntryDto {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  /** Bereits formatiert, z. B. "rwxr-xr-x" (s. `crate::dto::RemoteEntryDto`). */
  permissions: string;
  /** RFC3339, `null` wenn der Server keine Änderungszeit meldet. */
  modified: string | null;
}

export type SftpTransferKind = "upload" | "download";

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
}
