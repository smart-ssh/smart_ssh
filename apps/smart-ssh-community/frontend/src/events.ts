import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type * as Ev from "./types";

// Dünne, typisierte Wrapper um `@tauri-apps/api/event`'s `listen()` — ein
// Name pro Event aus Spec 0007 Abschnitt 5, damit Aufrufer nie den
// String-Namen von Hand tippen (Tippfehlerquelle) und die Payload-Form aus
// `types.ts` automatisch mitbekommen.

/** Subscribes to one backend event and hands only its payload to the handler. */
const payloadListener =
  <T>(name: string) =>
  (handler: (event: T) => void): Promise<UnlistenFn> =>
    listen<T>(name, (e) => handler(e.payload));

export const onConnectionStatusChanged =
  payloadListener<Ev.ConnectionStatusChangedEvent>("connection-status-changed");

export const onChatSessionChanged =
  payloadListener<Ev.ChatSessionChangedEvent>("chat-session-changed");

export const onAutoReconnectChanged =
  payloadListener<Ev.AutoReconnectChangedEvent>("auto-reconnect-changed");

export const onHostKeyVerificationNeeded =
  payloadListener<Ev.HostKeyVerificationNeededEvent>("host-key-verification-needed");

/** Issue #37: Ende einer Host-Key-Abfrage im Backend (Timeout, Abbruch oder
 * Entscheidung) — s. `HostKeyVerificationEndedEvent`. */
export const onHostKeyVerificationEnded =
  payloadListener<Ev.HostKeyVerificationEndedEvent>("host-key-verification-ended");

/** Issue #546: live step list of a running `connect`. */
export const onConnectProgress = payloadListener<Ev.ConnectProgressEvent>("connect-progress");

export const onTerminalOutput = payloadListener<Ev.TerminalOutputEvent>("terminal-output");

export const onChatTextDelta = payloadListener<Ev.ChatTextDeltaEvent>("chat-text-delta");

export const onChatActionProposed =
  payloadListener<Ev.ChatActionProposedEvent>("chat-action-proposed");

/** Spec 0026, Abschnitt 3, Punkt 4 — kommt (falls die KI-Zweitmeinung
 * aktiviert ist) nach `onChatActionProposed` für dieselbe `actionId`. */
export const onRiskAssessmentUpdated =
  payloadListener<Ev.RiskAssessmentUpdatedEvent>("risk-assessment-updated");

/** Spec 0092, A3.2 — s. `ActionDecisionEscalatedEvent`-Doc-Kommentar.
 * Kommt nach `onRiskAssessmentUpdated` für dieselbe `actionId`. */
export const onActionDecisionEscalated =
  payloadListener<Ev.ActionDecisionEscalatedEvent>("action-decision-escalated");

export const onChatActionResult = payloadListener<Ev.ChatActionResultEvent>("chat-action-result");

/** Issue #325 (Spec 0106) — live output of a running command. */
export const onChatActionOutput = payloadListener<Ev.ChatActionOutputEvent>("chat-action-output");

/** Issue #327 — a running command is probably waiting for input. */
export const onChatActionWaitingInput =
  payloadListener<Ev.ChatActionWaitingInputEvent>("chat-action-waiting-input");

export const onChatError = payloadListener<Ev.ChatErrorEvent>("chat-error");

/** Issue #694 — a request to the AI provider was sent (wait line). */
export const onChatAiRequestStarted =
  payloadListener<Ev.ChatAiRequestStartedEvent>("chat-ai-request-started");

/** Issue #600 — automatic retry after a provider overload. */
export const onChatAiRetrying = payloadListener<Ev.ChatAiRetryingEvent>("chat-ai-retrying");

/** Spec 0065, Teil 2 — s. `ChatResponseTruncatedEvent`-Doc-Kommentar. */
export const onChatResponseTruncated =
  payloadListener<Ev.ChatResponseTruncatedEvent>("chat-response-truncated");

/** Issue #601 — real context size after a response. */
export const onChatContextUsage = payloadListener<Ev.ChatContextUsageEvent>("chat-context-usage");

/** Issue #601 — one event per compaction step. */
export const onChatContextCompacted =
  payloadListener<Ev.ChatContextCompactedEvent>("chat-context-compacted");

/** Spec 0105 — eine serverseitige Web-Recherche des Providers. */
export const onChatWebActivity = payloadListener<Ev.ChatWebActivityEvent>("chat-web-activity");

/** Issue #693 — progress/verdict of the injection check. */
export const onChatInjectionCheck =
  payloadListener<Ev.ChatInjectionCheckEvent>("chat-injection-check");

/** Spec 0105 §7 (Issue #169) — s. `ChatWebResearchUnavailableEvent`. */
export const onChatWebResearchUnavailable =
  payloadListener<Ev.ChatWebResearchUnavailableEvent>("chat-web-research-unavailable");

/** Spec 0080, A2 — s. `ChatResponseEmptyEvent`-Doc-Kommentar. */
export const onChatResponseEmpty =
  payloadListener<Ev.ChatResponseEmptyEvent>("chat-response-empty");

/** Issue #647 — s. `ChatResponseRefusedEvent`-Doc-Kommentar. */
export const onChatResponseRefused =
  payloadListener<Ev.ChatResponseRefusedEvent>("chat-response-refused");

/** Spec 0066, §1 — s. `ChatResponseCancelledEvent`-Doc-Kommentar. */
export const onChatResponseCancelled =
  payloadListener<Ev.ChatResponseCancelledEvent>("chat-response-cancelled");

/** Spec 0066, §2 — s. `ChatQueuedMessagesSentEvent`-Doc-Kommentar. */
export const onChatQueuedMessagesSent =
  payloadListener<Ev.ChatQueuedMessagesSentEvent>("chat-queued-messages-sent");

/** Spec 0021, Abschnitt 4 / `docs/adr/0021-turn-continuation-design.md`:
 * das Automatik-Cap ist ein weicher Stopp, kein Fehler — eigenes Event statt
 * über `onChatError`, damit das Frontend es neutral statt als Fehler-Karte
 * anzeigen kann. */
export const onChatAutoContinuationLimitReached =
  payloadListener<Ev.ChatAutoContinuationLimitReachedEvent>("chat-auto-continuation-limit-reached");

/** Spec 0021, Abschnitt 5 — s. `ChatAutoContinuationStartedEvent`-Doc-
 * Kommentar. */
export const onChatAutoContinuationStarted =
  payloadListener<Ev.ChatAutoContinuationStartedEvent>("chat-auto-continuation-started");

/** Spec 0061, Abschnitt 4 — s. `AiBudgetWaitingEvent`-Doc-Kommentar. */
export const onAiBudgetWaiting = payloadListener<Ev.AiBudgetWaitingEvent>("ai-budget-waiting");

/** Spec 0010 — bewusst app-weit abonniert (z. B. in `App.tsx`), nicht nur
 * innerhalb einer offenen `ChatPanel`-Instanz: der Vorschlag kann eintreffen,
 * nachdem der Nutzer den Session-Screen bereits verlassen hat. */
export const onNoteUpdateSuggested =
  payloadListener<Ev.NoteUpdateSuggestedEvent>("note-update-suggested");

/** Spec 0057, §4.2 (Etappe 4) — der ERSTE Dialog, bevor überhaupt ein
 * KI-Aufruf stattgefunden hat. App-weit, aus demselben Grund wie
 * `onNoteUpdateSuggested`. */
export const onNoteShrinkSuggested =
  payloadListener<Ev.NoteShrinkSuggestedEvent>("note-shrink-suggested");

/** Spec 0057, §4.2/§6 — der KI-Aufruf hinter "Ja, zusammenfassen" ist
 * fehlgeschlagen oder abgelaufen; die gespeicherte Notiz blieb unverändert. */
export const onNoteShrinkFailed = payloadListener<Ev.NoteShrinkFailedEvent>("note-shrink-failed");

/** Spec 0058 (Politur-Paket) — das Erfolgs-Gegenstück, s.
 * `NoteShrinkSucceededEvent`-Doc-Kommentar. */
export const onNoteShrinkSucceeded =
  payloadListener<Ev.NoteShrinkSucceededEvent>("note-shrink-succeeded");

/** Spec 0012 — läuft, anders als `onChatActionProposed`, nie durch einen
 * Bestätigungsdialog (s. `ChatDocumentGeneratedEvent`-Doc-Kommentar). */
export const onChatDocumentGenerated =
  payloadListener<Ev.ChatDocumentGeneratedEvent>("chat-document-generated");

/** Issue #545: the AI asked a question with answer options (no UI yet). */
export const onChatQuestionAsked =
  payloadListener<Ev.ChatQuestionAskedEvent>("chat-question-asked");

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser ---------------------

export const onSftpTransferStarted =
  payloadListener<Ev.SftpTransferStartedEvent>("sftp-transfer-started");

export const onSftpTransferProgress =
  payloadListener<Ev.SftpTransferProgressEvent>("sftp-transfer-progress");

export const onSftpTransferFinished =
  payloadListener<Ev.SftpTransferFinishedEvent>("sftp-transfer-finished");

// --- Spec 0028: MCP-Server-Integration -----------------------------------

/** Spec 0028, Abschnitt 9a — signalisiert, für `serverId` einen Tab zu
 * öffnen/zu diesem zu wechseln, bevor ein Host-Key- oder Bestätigungsdialog
 * für diese Session erscheint. Nur für die vier aktionsauslösenden Tools. */
export const onMcpActionTabRequested =
  payloadListener<Ev.McpActionTabRequestedEvent>("mcp-action-tab-requested");

// --- Spec 0101, Etappe 3: Startfragen im Fenster -------------------------

/** §2 Frage 3: eine Startfrage (D1–D4, zweite Bestätigung, neues
 * Master-Passwort, Hinweis) erscheint im Fenster.
 *
 * **Der Zuhörer muss stehen, bevor das erste Entsperr-Kommando läuft.**
 * `emit` im Backend liefert `Ok`, auch wenn niemand zuhört: Geht das
 * Ereignis verloren, läuft die Frage dort in die Zeitgrenze von fünf
 * Minuten (s. `crate::window_prompt::PROMPT_TIMEOUT`) und der Start bricht
 * mit „keine Antwort erhalten" ab. */
export const onStartupPrompt = payloadListener<Ev.StartupPromptRequest>("startup:prompt");

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
export const onNoteUpdateExpired =
  payloadListener<Ev.NoteUpdateExpiredEvent>("note-update-expired");
