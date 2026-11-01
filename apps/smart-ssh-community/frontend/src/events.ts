import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ActionDecisionEscalatedEvent,
  AiBudgetWaitingEvent,
  ChatActionOutputEvent,
  ChatActionProposedEvent,
  ChatActionResultEvent,
  ChatActionWaitingInputEvent,
  ChatAutoContinuationLimitReachedEvent,
  ChatAutoContinuationStartedEvent,
  ChatDocumentGeneratedEvent,
  ChatQuestionAskedEvent,
  ChatErrorEvent,
  ChatAiRetryingEvent,
  ChatAiRequestStartedEvent,
  ChatQueuedMessagesSentEvent,
  ChatResponseCancelledEvent,
  ChatResponseEmptyEvent,
  ChatResponseRefusedEvent,
  ChatResponseTruncatedEvent,
  ChatContextUsageEvent,
  ChatContextCompactedEvent,
  ChatTextDeltaEvent,
  ChatWebActivityEvent,
  ChatInjectionCheckEvent,
  ChatWebResearchUnavailableEvent,
  AutoReconnectChangedEvent,
  ChatSessionChangedEvent,
  ConnectionStatusChangedEvent,
  ConnectProgressEvent,
  HostKeyVerificationEndedEvent,
  HostKeyVerificationNeededEvent,
  McpActionTabRequestedEvent,
  NoteShrinkFailedEvent,
  NoteShrinkSucceededEvent,
  NoteShrinkSuggestedEvent,
  NoteUpdateExpiredEvent,
  NoteUpdateSuggestedEvent,
  RiskAssessmentUpdatedEvent,
  SftpTransferFinishedEvent,
  SftpTransferProgressEvent,
  SftpTransferStartedEvent,
  StartupPromptRequest,
  TerminalOutputEvent,
} from "./types";

// Dünne, typisierte Wrapper um `@tauri-apps/api/event`'s `listen()` — ein
// Name pro Event aus Spec 0007 Abschnitt 5, damit Aufrufer nie den
// String-Namen von Hand tippen (Tippfehlerquelle) und die Payload-Form aus
// `types.ts` automatisch mitbekommen.

/** Subscribes to one backend event and hands only its payload to the handler. */
const payloadListener =
  <T>(name: string) =>
  (handler: (event: T) => void): Promise<UnlistenFn> =>
    listen<T>(name, (e) => handler(e.payload));

export const onConnectionStatusChanged = payloadListener<ConnectionStatusChangedEvent>(
  "connection-status-changed",
);

export const onChatSessionChanged =
  payloadListener<ChatSessionChangedEvent>("chat-session-changed");

export const onAutoReconnectChanged =
  payloadListener<AutoReconnectChangedEvent>("auto-reconnect-changed");

export const onHostKeyVerificationNeeded = payloadListener<HostKeyVerificationNeededEvent>(
  "host-key-verification-needed",
);

/** Issue #37: Ende einer Host-Key-Abfrage im Backend (Timeout, Abbruch oder
 * Entscheidung) — s. `HostKeyVerificationEndedEvent`. */
export const onHostKeyVerificationEnded = payloadListener<HostKeyVerificationEndedEvent>(
  "host-key-verification-ended",
);

/** Issue #546: live step list of a running `connect`. */
export const onConnectProgress = payloadListener<ConnectProgressEvent>("connect-progress");

export const onTerminalOutput = payloadListener<TerminalOutputEvent>("terminal-output");

export const onChatTextDelta = payloadListener<ChatTextDeltaEvent>("chat-text-delta");

export const onChatActionProposed =
  payloadListener<ChatActionProposedEvent>("chat-action-proposed");

/** Spec 0026, Abschnitt 3, Punkt 4 — kommt (falls die KI-Zweitmeinung
 * aktiviert ist) nach `onChatActionProposed` für dieselbe `actionId`. */
export const onRiskAssessmentUpdated =
  payloadListener<RiskAssessmentUpdatedEvent>("risk-assessment-updated");

/** Spec 0092, A3.2 — s. `ActionDecisionEscalatedEvent`-Doc-Kommentar.
 * Kommt nach `onRiskAssessmentUpdated` für dieselbe `actionId`. */
export const onActionDecisionEscalated = payloadListener<ActionDecisionEscalatedEvent>(
  "action-decision-escalated",
);

export const onChatActionResult = payloadListener<ChatActionResultEvent>("chat-action-result");

/** Issue #325 (Spec 0106) — live output of a running command. */
export const onChatActionOutput = payloadListener<ChatActionOutputEvent>("chat-action-output");

/** Issue #327 — a running command is probably waiting for input. */
export const onChatActionWaitingInput = payloadListener<ChatActionWaitingInputEvent>(
  "chat-action-waiting-input",
);

export const onChatError = payloadListener<ChatErrorEvent>("chat-error");

/** Issue #694 — a request to the AI provider was sent (wait line). */
export const onChatAiRequestStarted =
  payloadListener<ChatAiRequestStartedEvent>("chat-ai-request-started");

/** Issue #600 — automatic retry after a provider overload. */
export const onChatAiRetrying = payloadListener<ChatAiRetryingEvent>("chat-ai-retrying");

/** Spec 0065, Teil 2 — s. `ChatResponseTruncatedEvent`-Doc-Kommentar. */
export const onChatResponseTruncated =
  payloadListener<ChatResponseTruncatedEvent>("chat-response-truncated");

/** Issue #601 — real context size after a response. */
export const onChatContextUsage = payloadListener<ChatContextUsageEvent>("chat-context-usage");

/** Issue #601 — one event per compaction step. */
export const onChatContextCompacted =
  payloadListener<ChatContextCompactedEvent>("chat-context-compacted");

/** Spec 0105 — eine serverseitige Web-Recherche des Providers. */
export const onChatWebActivity = payloadListener<ChatWebActivityEvent>("chat-web-activity");

/** Issue #693 — progress/verdict of the injection check. */
export const onChatInjectionCheck =
  payloadListener<ChatInjectionCheckEvent>("chat-injection-check");

/** Spec 0105 §7 (Issue #169) — s. `ChatWebResearchUnavailableEvent`. */
export const onChatWebResearchUnavailable = payloadListener<ChatWebResearchUnavailableEvent>(
  "chat-web-research-unavailable",
);

/** Spec 0080, A2 — s. `ChatResponseEmptyEvent`-Doc-Kommentar. */
export const onChatResponseEmpty = payloadListener<ChatResponseEmptyEvent>("chat-response-empty");

/** Issue #647 — s. `ChatResponseRefusedEvent`-Doc-Kommentar. */
export const onChatResponseRefused =
  payloadListener<ChatResponseRefusedEvent>("chat-response-refused");

/** Spec 0066, §1 — s. `ChatResponseCancelledEvent`-Doc-Kommentar. */
export const onChatResponseCancelled =
  payloadListener<ChatResponseCancelledEvent>("chat-response-cancelled");

/** Spec 0066, §2 — s. `ChatQueuedMessagesSentEvent`-Doc-Kommentar. */
export const onChatQueuedMessagesSent = payloadListener<ChatQueuedMessagesSentEvent>(
  "chat-queued-messages-sent",
);

/** Spec 0021, Abschnitt 4 / `docs/adr/0021-turn-continuation-design.md`:
 * das Automatik-Cap ist ein weicher Stopp, kein Fehler — eigenes Event statt
 * über `onChatError`, damit das Frontend es neutral statt als Fehler-Karte
 * anzeigen kann. */
export const onChatAutoContinuationLimitReached =
  payloadListener<ChatAutoContinuationLimitReachedEvent>("chat-auto-continuation-limit-reached");

/** Spec 0021, Abschnitt 5 — s. `ChatAutoContinuationStartedEvent`-Doc-
 * Kommentar. */
export const onChatAutoContinuationStarted = payloadListener<ChatAutoContinuationStartedEvent>(
  "chat-auto-continuation-started",
);

/** Spec 0061, Abschnitt 4 — s. `AiBudgetWaitingEvent`-Doc-Kommentar. */
export const onAiBudgetWaiting = payloadListener<AiBudgetWaitingEvent>("ai-budget-waiting");

/** Spec 0010 — bewusst app-weit abonniert (z. B. in `App.tsx`), nicht nur
 * innerhalb einer offenen `ChatPanel`-Instanz: der Vorschlag kann eintreffen,
 * nachdem der Nutzer den Session-Screen bereits verlassen hat. */
export const onNoteUpdateSuggested =
  payloadListener<NoteUpdateSuggestedEvent>("note-update-suggested");

/** Spec 0057, §4.2 (Etappe 4) — der ERSTE Dialog, bevor überhaupt ein
 * KI-Aufruf stattgefunden hat. App-weit, aus demselben Grund wie
 * `onNoteUpdateSuggested`. */
export const onNoteShrinkSuggested =
  payloadListener<NoteShrinkSuggestedEvent>("note-shrink-suggested");

/** Spec 0057, §4.2/§6 — der KI-Aufruf hinter "Ja, zusammenfassen" ist
 * fehlgeschlagen oder abgelaufen; die gespeicherte Notiz blieb unverändert. */
export const onNoteShrinkFailed = payloadListener<NoteShrinkFailedEvent>("note-shrink-failed");

/** Spec 0058 (Politur-Paket) — das Erfolgs-Gegenstück, s.
 * `NoteShrinkSucceededEvent`-Doc-Kommentar. */
export const onNoteShrinkSucceeded =
  payloadListener<NoteShrinkSucceededEvent>("note-shrink-succeeded");

/** Spec 0012 — läuft, anders als `onChatActionProposed`, nie durch einen
 * Bestätigungsdialog (s. `ChatDocumentGeneratedEvent`-Doc-Kommentar). */
export const onChatDocumentGenerated =
  payloadListener<ChatDocumentGeneratedEvent>("chat-document-generated");

/** Issue #545: the AI asked a question with answer options (no UI yet). */
export const onChatQuestionAsked = payloadListener<ChatQuestionAskedEvent>("chat-question-asked");

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser ---------------------

export const onSftpTransferStarted =
  payloadListener<SftpTransferStartedEvent>("sftp-transfer-started");

export const onSftpTransferProgress =
  payloadListener<SftpTransferProgressEvent>("sftp-transfer-progress");

export const onSftpTransferFinished =
  payloadListener<SftpTransferFinishedEvent>("sftp-transfer-finished");

// --- Spec 0028: MCP-Server-Integration -----------------------------------

/** Spec 0028, Abschnitt 9a — signalisiert, für `serverId` einen Tab zu
 * öffnen/zu diesem zu wechseln, bevor ein Host-Key- oder Bestätigungsdialog
 * für diese Session erscheint. Nur für die vier aktionsauslösenden Tools. */
export const onMcpActionTabRequested = payloadListener<McpActionTabRequestedEvent>(
  "mcp-action-tab-requested",
);

// --- Spec 0101, Etappe 3: Startfragen im Fenster -------------------------

/** §2 Frage 3: eine Startfrage (D1–D4, zweite Bestätigung, neues
 * Master-Passwort, Hinweis) erscheint im Fenster.
 *
 * **Der Zuhörer muss stehen, bevor das erste Entsperr-Kommando läuft.**
 * `emit` im Backend liefert `Ok`, auch wenn niemand zuhört: Geht das
 * Ereignis verloren, läuft die Frage dort in die Zeitgrenze von fünf
 * Minuten (s. `crate::window_prompt::PROMPT_TIMEOUT`) und der Start bricht
 * mit „keine Antwort erhalten" ab. */
export const onStartupPrompt = payloadListener<StartupPromptRequest>("startup:prompt");

/** A16: Der Zustand steht jetzt — die Oberfläche darf die App zeigen. */
export const onStartupUnlocked = (handler: () => void): Promise<UnlistenFn> =>
  listen<null>("startup:unlocked", () => handler());

/** Base64 → `Uint8Array`, für `TerminalOutputEvent.data` (s. `crate::events`). */
export function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/** Issue #628: the backend timed out a pending note confirmation (update or
 * shrink suggestion); the card switches to its "expired" state. */
export const onNoteUpdateExpired = payloadListener<NoteUpdateExpiredEvent>("note-update-expired");
