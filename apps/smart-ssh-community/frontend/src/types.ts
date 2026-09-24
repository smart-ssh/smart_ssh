// Spiegelt die Rust-DTOs aus `crates/app-shell/src/dto.rs` (Spec 0007,
// Abschnitt 8.2). Feldnamen camelCase, weil die DTO-Structs dort
// `#[serde(rename_all = "camelCase")]` tragen; `ProviderType`-Werte
// bleiben snake_case (exakt wie im SQL-`CHECK`-Constraint), weil dieser
// eine Typ stattdessen `#[serde(rename = "...")]` pro Variante nutzt.

export type ProviderType =
  | "openai"
  | "anthropic"
  | "generic_openai_compatible"
  | "ollama";

export type AuthMethodKind = "password" | "private_key" | "agent" | "certificate" | "identity_file";

/** Spec 0039, Abschnitt 5.1 — steuert NICHT das Fencing selbst (in jeder
 * Stufe aktiv), nur ob/wann zusätzlich auf Bestätigung eskaliert wird,
 * nachdem in der Sitzung Serverinhalt eingelesen wurde. */
export type PostIngestPolicy = "strict" | "balanced" | "standard";

export interface ServerDto {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  groupId: string | null;
  tags: string[];
  authKind: AuthMethodKind;
  /** Spec 0076, B-4: **wo** die Schlüsseldatei liegt — `null` bei jeder
   * anderen Anmeldeart. Kein Geheimnis, deshalb Teil dieses DTOs (s.
   * `crate::dto::ServerDto::identity_file_path`-Doc-Kommentar). */
  identityFilePath: string | null;
  jumpHost: string | null;
  notes: string;
  /** Spec 0018, Abschnitt 4: ob ein Sudo-Passwort im Schlüsselbund
   * hinterlegt ist — nie der Wert selbst. */
  hasSudoPassword: boolean;
  /** Spec 0071, A14: `true`, wenn der Systemschlüsselbund nicht sagen
   * konnte, ob ein Sudo-Passwort hinterlegt ist. Dann ist
   * `hasSudoPassword` ebenfalls `false` — dieses Feld muss zuerst geprüft
   * werden, sonst behauptet die Oberfläche "kein Sudo-Passwort
   * hinterlegt", obwohl sie es nicht weiß. */
  sudoPasswordUnknown: boolean;

  /** Spec 0032, Abschnitt 3: `true` genau für den lokalen Pseudo-Server. */
  isLocal: boolean;
  postIngestPolicy: PostIngestPolicy;
  /** Spec 0039, Abschnitt 5.2: nur wirksam, wenn zusätzlich eine
   * Zweitmeinungs-Provider-Konfiguration vorhanden ist (s.
   * `loadRiskClassifierSettings` in `riskSettings.ts`). */
  aiInjectionCheckEnabled: boolean;
  /** Spec 0067, A2: Override für den `sftp-server`-Pfad im erhöhten
   * Dateibrowser; `null` = automatisch erkennen. */
  sftpServerPath: string | null;
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
  /** Spec 0065, Teil 4: `null` = „Automatisch" (Default). */
  maxTokensOverride: number | null;
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
  /** Spec 0065, Teil 4: `null` = „Automatisch" — nur relevant für den
   * Haupt-Chat dieses Providers, Nebenaufrufe behalten ihre kleinen Werte. */
  maxTokensOverride: number | null;
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

/** `code` (Spec 0024, Abschnitt 5): stabiler, sprachunabhängiger Bezeichner
 * der Grund-Art, fürs Mapping auf einen Übersetzungs-Key (s.
 * `errorCodes.ts`) — `reason` bleibt der bestehende (deutsche) Anzeigetext,
 * unverändert als Fallback, falls ein `code` nicht gemappt ist. */
export type Decision =
  | "AutoExec"
  | { Confirm: { reason: string; code: string } }
  | { Deny: { reason: string; code: string } };

/** Spec 0028, Abschnitt 6/9a: Ursprung eines vorgeschlagenen Kommandos —
 * `mcp` zeigt im Bestätigungsdialog "Angefragt über: <clientName oder
 * generischer Text>" statt des sonst gezeigten KI-Provider-Namens.
 * `clientName` ist der optionale `clientInfo.name` aus dem MCP-Handshake,
 * `null` falls der verbindende Client keinen übermittelt hat. */
export type ActionOrigin = { kind: "internal" } | { kind: "mcp"; clientName: string | null };

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
  /** Spec 0028, Abschnitt 6/9a. */
  origin: ActionOrigin;
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

export interface ChatActionResultEvent {
  sessionId: string;
  actionId: string;
  result: ActionResultPayload;
}

export interface ChatErrorEvent {
  sessionId: string;
  message: string;
  /** Spec 0024, Abschnitt 5 — stabiler Backend-Code (`SshError`/`AiError`),
   * `null`/`undefined` wenn keiner vorliegt (reine Validierungsmeldung ohne
   * typisierten Fehler dahinter). Übersetzung via `translateErrorCode`. */
  code?: string | null;
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

/** Vorschau/Ergebnis von `deleteServer` (Spec 0046, Fund 1) — `server`
 * trägt bereits `authKind`/`hasSudoPassword`, daraus lässt sich ableiten,
 * welche Keychain-Secrets beim Löschen entfernt würden. */
export interface DeleteServerResult {
  server: ServerDto;
  serversLosingJumpHost: ServerDto[];
  executed: boolean;
  /** Spec 0071, A17: `CredentialRef`-Strings der Secrets, die beim Löschen
   * **nicht** aus dem Schlüsselbund entfernt werden konnten. Der Server ist
   * trotzdem gelöscht — die Einträge sind danach verwaist und müssen von
   * Hand entfernt werden. Leer im Normalfall und immer leer bei
   * `executed: false`. Kein Secret, nur der Account-Name im Schlüsselbund. */
  secretsLeftBehind: string[];
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
  postIngestPolicy: PostIngestPolicy;
  aiInjectionCheckEnabled: boolean;
  /** Spec 0067, A2: leer/`null` = automatisch. */
  sftpServerPath: string | null;
}

export type AuthMethodInput =
  | { kind: "password"; value: string | null }
  | { kind: "privateKey"; keyContent: string | null; passphrase: string | null }
  | { kind: "agent" }
  | { kind: "certificate"; certContent: string | null; keyContent: string | null }
  /** Spec 0076, A-1/B-2 — anders als bei den übrigen Varianten ist `path`
   * kein Secret und nicht optional (s. `crate::dto::AuthMethodInput::
   * IdentityFile`-Doc-Kommentar: „leer = unverändert" gilt für
   * Schlüsselbund-Slots, nicht für dieses Klartextfeld). `passphrase`
   * verhält sich dagegen wie bei `privateKey` (A-5). */
  | { kind: "identityFile"; path: string; passphrase: string | null };

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
  | {
      kind: "networkError";
      message: string;
      /** Spec 0069, Teil A4/E3: additiv, optional — `null`/`undefined`
       * fällt im Frontend auf `message` zurück. */
      code?: string | null;
    }
  | { kind: "timeout" };

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
  /** Spec 0077, 3.2.3: Fehlertext, wenn sich das Muster dieser Regel nicht
   * übersetzen lässt — sonst `null`. Eine solche Regel kann bei der
   * Auswertung nicht greifen, soweit ihr Muster nicht übersetzt; die Liste
   * markiert sie deshalb sichtbar. Kann bei einer Regel entstehen, die eine
   * ältere Programmfassung gespeichert hat; neu angelegt werden kann sie
   * nicht mehr (Schicht 1). */
  patternError: string | null;
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

/** Spec 0037, Abschnitt 4: Word-Export komplett entfernt (kein Gating) —
 * nur noch Markdown. */
export type DocumentFormat = "markdown";

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

/** Spec 0034, Abschnitt 6/8: Eintrag im Sitzungs-Auswahl-Screen. */
export interface ChatSessionSummaryDto {
  sessionId: string;
  title: string | null;
  startedAt: string;
  endedAt: string | null;
  messageCount: number;
}

/** Spec 0034, Abschnitt 6/8: bereits geladene Historie eines Tabs (leer bei
 * `connect()`, befüllt nach `resume_chat_session()`), s.
 * `get_chat_history`. */
export type ChatHistoryRole = "user" | "assistant" | "action_result";

export type ChatHistoryEntryDto =
  | { type: "text"; role: ChatHistoryRole; text: string }
  | {
      type: "commandResult";
      role: ChatHistoryRole;
      command: string;
      stdout: string;
      stderr: string;
      exitCode: number | null;
      cancelled: boolean;
      /** Spec 0043, Fund A — s. `ActionResultPayload`'s `truncated`. */
      truncated: boolean;
    }
  | { type: "actionRejected"; role: ChatHistoryRole; command: string; reason: string };

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
  /** Spec 0054, Teil 2: dieselben Bits wie `permissions`, numerisch
   * (`0o644`-Stil) statt als `rwxr-xr-x`-String — Eigenschaften-Dialog und
   * Grundlage für den chmod-Dialog (Teil 3). */
  permissionsOctal: number;
  /** `null`, wenn der Server keine numerische uid/gid liefert (selten). */
  uid: number | null;
  gid: number | null;
  /** `null`, sofern der Server keine Namensauflösung liefert (die meisten
   * SFTPv3-Server nicht — dann bleibt nur `uid`/`gid` numerisch). */
  owner: string | null;
  group: string | null;
}

/** Spec 0054, Teil 3: Vorschau vor dem Löschen eines Ordners. */
/** Spec 0067, A3: `crate::dto::ElevationFailureKind`. */
export type ElevationFailureKind =
  | "unsupported"
  | "invalidUser"
  | "invalidPath"
  | "sftpServerNotFound"
  | "passwordRequired"
  | "notAllowed"
  | "requireTty"
  | "sudoMissing"
  | "checkFailed"
  | "startFailed";

/** Spec 0067, A3: Ergebnis von `sftp_elevation_enable`. */
export interface ElevationResultDto {
  active: boolean;
  targetUser: string;
  sftpServerPath: string | null;
  failure: {
    kind: ElevationFailureKind;
    /** Zugeschnittene sudoers-Zeile zum Kopieren, falls eine Regel fehlt. */
    sudoersLine: string | null;
    detail: string | null;
  } | null;
}

/** Spec 0067, Teil B: `crate::dto::DownloadResultDto`. */
export interface DownloadResultDto {
  localPath: string;
  isDir: boolean;
  fileCount: number;
}

export interface DeletePreviewDto {
  fileCount: number;
  dirCount: number;
}

/** Spec 0054, Teil 3: die lokale Seite der Upload-Überschreib-Diff-Vorschau
 * — `text: null` bei einer zu großen/nicht-Text-Datei. */
export interface LocalFilePreviewDto {
  text: string | null;
  size: number;
}

/** Spec 0054, Teil 4: Ergebnis von `sftpOpenForEditing`. */
export interface EditSessionDto {
  localPath: string;
  /** RFC3339, `null` wenn der Server keine Änderungszeit meldet. */
  remoteModified: string | null;
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

// --- Spec 0028: MCP-Server-Integration -----------------------------------

/** Spec 0028, Abschnitt 9a: signalisiert, für `serverId` einen Tab zu
 * öffnen (falls noch keiner existiert) bzw. zu diesem zu wechseln — nur
 * für die vier aktionsauslösenden MCP-Tools, nicht für `list_servers`/
 * `get_server_notes`. */
export interface McpActionTabRequestedEvent {
  sessionId: string;
  serverId: string;
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

/** Von `crate::dto::AppInfoDto`. */
export interface AppInfoDto {
  /** Aus `tauri.conf.json` (Spec 0048), z. B. `"0.4.1"`. */
  version: string;
  /** Kurzer Git-Commit-Hash, z. B. `"a5b3e01"`, oder `"unknown"` ohne Git
   * zur Build-Zeit. */
  commitHash: string;
  /** Das geteilte Anzeigeformat: `"0.4.1 (a5b3e01)"`. */
  versionDisplay: string;
  /** `"Community"` oder `"Official"`. */
  edition: string;
  /** `"Dev"` (Debug-Build, eigenes Datenverzeichnis) oder `"Release"`. */
  buildType: "Dev" | "Release";
}

// --- Spec 0071: Systemschlüsselbund-Zustand -------------------------------

/** Warum der Systemschlüsselbund nicht verfügbar ist — von
 * `crate::dto::KeychainUnavailableReasonDto` (Spec 0071, A1). */
export type KeychainUnavailableReason =
  | "no_session_bus"
  | "no_secret_service_provider"
  | "locked"
  | "unknown";

/** Von `crate::dto::KeychainStatusDto` (Spec 0071, A15). Trägt bewusst
 * keinen Fehlertext — die anzeigbaren Texte stehen im
 * Übersetzungskatalog. */
export interface KeychainStatusDto {
  available: boolean;
  /** `null`, wenn `available`. */
  reason: KeychainUnavailableReason | null;
}

// --- Spec 0076: Anmeldung mit einer Schlüsseldatei -----------------------

/** Der Grund aus `KeyFileFactsDto.problem` (`crate::dto::
 * KeyFileProblemDto`) — `code` ist ein stabiler Bezeichner (Spec 0024,
 * Abschnitt 5, s. `errorCodes.ts`), `message` der deutsche Fallback-Text,
 * falls `code` einmal nicht bekannt ist. Trägt nie Dateiinhalt (§5.2). */
export interface KeyFileProblemDto {
  code: string;
  message: string;
}

/** Von `crate::dto::KeyFileFactsDto` (Spec 0076, B-3/C-7) — der
 * Vorab-Befund über eine Schlüsseldatei, ohne je den Schlüssel selbst
 * herauszugeben (§4.2, §5.2). */
export interface KeyFileFactsDto {
  exists: boolean;
  /** Nur Unix; auf Windows immer `false` (A-4). */
  permissionsTooOpen: boolean;
  validKey: boolean;
  encrypted: boolean;
  /** `null`, wenn die Datei grundsätzlich in Frage kommt. */
  problem: KeyFileProblemDto | null;
}
