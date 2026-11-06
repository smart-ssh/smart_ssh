import type { UnlistenFn } from "@tauri-apps/api/event";
import { EVENT } from "./eventNames";
import type { EventPayloads } from "./eventPayloads";
import { listenTyped } from "./listenTyped";

export { listenTyped };

// Dünne, typisierte Wrapper um `@tauri-apps/api/event`'s `listen()` — ein
// Name pro Event aus Spec 0007 Abschnitt 5, damit Aufrufer nie den
// String-Namen von Hand tippen (Tippfehlerquelle) und die Payload-Form aus
// `types.ts` automatisch mitbekommen.

/** `listenTyped` for one event name, as a reusable `on…` subscription. */
const typedListener =
  <K extends keyof EventPayloads>(name: K) =>
  (handler: (payload: EventPayloads[K]) => void): Promise<UnlistenFn> =>
    listenTyped(name, handler);

export const onConnectionStatusChanged =
  typedListener(EVENT.CONNECTION_STATUS_CHANGED);

export const onChatSessionChanged =
  typedListener(EVENT.CHAT_SESSION_CHANGED);

export const onAutoReconnectChanged =
  typedListener(EVENT.AUTO_RECONNECT_CHANGED);

export const onHostKeyVerificationNeeded =
  typedListener(EVENT.HOST_KEY_VERIFICATION_NEEDED);

/** Issue #37: Ende einer Host-Key-Abfrage im Backend (Timeout, Abbruch oder
 * Entscheidung) — s. `HostKeyVerificationEndedEvent`. */
export const onHostKeyVerificationEnded =
  typedListener(EVENT.HOST_KEY_VERIFICATION_ENDED);

/** Issue #1185: the server asks for login input (password, code, …). */
export const onAuthPromptNeeded = typedListener(EVENT.AUTH_PROMPT_NEEDED);

export const onAuthPromptEnded = typedListener(EVENT.AUTH_PROMPT_ENDED);

/** Issue #546: live step list of a running `connect`. */
export const onConnectProgress = typedListener(EVENT.CONNECT_PROGRESS);

/** Issue #1173: an established connection uses outdated SSH algorithms. */
export const onConnectWarnings = typedListener(EVENT.CONNECT_WARNINGS);

export const onTerminalOutput = typedListener(EVENT.TERMINAL_OUTPUT);

export const onChatTextDelta = typedListener(EVENT.CHAT_TEXT_DELTA);

export const onChatActionProposed =
  typedListener(EVENT.CHAT_ACTION_PROPOSED);

/** Spec 0026, Abschnitt 3, Punkt 4 — kommt (falls die KI-Zweitmeinung
 * aktiviert ist) nach `onChatActionProposed` für dieselbe `actionId`. */
export const onRiskAssessmentUpdated =
  typedListener(EVENT.RISK_ASSESSMENT_UPDATED);

/** Spec 0092, A3.2 — s. `ActionDecisionEscalatedEvent`-Doc-Kommentar.
 * Kommt nach `onRiskAssessmentUpdated` für dieselbe `actionId`. */
export const onActionDecisionEscalated =
  typedListener(EVENT.ACTION_DECISION_ESCALATED);

export const onChatActionResult = typedListener(EVENT.CHAT_ACTION_RESULT);

/** Issue #325 (Spec 0106) — live output of a running command. */
export const onChatActionOutput = typedListener(EVENT.CHAT_ACTION_OUTPUT);

/** Issue #327 — a running command is probably waiting for input. */
export const onChatActionWaitingInput =
  typedListener(EVENT.CHAT_ACTION_WAITING_INPUT);

export const onChatError = typedListener(EVENT.CHAT_ERROR);

/** Issue #694 — a request to the AI provider was sent (wait line). */
export const onChatAiRequestStarted =
  typedListener(EVENT.CHAT_AI_REQUEST_STARTED);

/** Issue #600 — automatic retry after a provider overload. */
export const onChatAiRetrying = typedListener(EVENT.CHAT_AI_RETRYING);

/** Spec 0065, Teil 2 — s. `ChatResponseTruncatedEvent`-Doc-Kommentar. */
export const onChatResponseTruncated =
  typedListener(EVENT.CHAT_RESPONSE_TRUNCATED);

/** Issue #601 — real context size after a response. */
export const onChatContextUsage = typedListener(EVENT.CHAT_CONTEXT_USAGE);

/** Issue #601 — one event per compaction step. */
export const onChatContextCompacted =
  typedListener(EVENT.CHAT_CONTEXT_COMPACTED);

/** Spec 0105 — eine serverseitige Web-Recherche des Providers. */
export const onChatWebActivity = typedListener(EVENT.CHAT_WEB_ACTIVITY);

/** Issue #693 — progress/verdict of the injection check. */
export const onChatInjectionCheck =
  typedListener(EVENT.CHAT_INJECTION_CHECK);

/** Spec 0105 §7 (Issue #169) — s. `ChatWebResearchUnavailableEvent`. */
export const onChatWebResearchUnavailable =
  typedListener(EVENT.CHAT_WEB_RESEARCH_UNAVAILABLE);

/** Spec 0080, A2 — s. `ChatResponseEmptyEvent`-Doc-Kommentar. */
export const onChatResponseEmpty =
  typedListener(EVENT.CHAT_RESPONSE_EMPTY);

/** Issue #647 — s. `ChatResponseRefusedEvent`-Doc-Kommentar. */
export const onChatResponseRefused =
  typedListener(EVENT.CHAT_RESPONSE_REFUSED);

/** Spec 0066, §1 — s. `ChatResponseCancelledEvent`-Doc-Kommentar. */
export const onChatResponseCancelled =
  typedListener(EVENT.CHAT_RESPONSE_CANCELLED);

/** Spec 0066, §2 — s. `ChatQueuedMessagesSentEvent`-Doc-Kommentar. */
export const onChatQueuedMessagesSent =
  typedListener(EVENT.CHAT_QUEUED_MESSAGES_SENT);

/** Spec 0021, Abschnitt 4 / `docs/adr/0021-turn-continuation-design.md`:
 * das Automatik-Cap ist ein weicher Stopp, kein Fehler — eigenes Event statt
 * über `onChatError`, damit das Frontend es neutral statt als Fehler-Karte
 * anzeigen kann. */
export const onChatAutoContinuationLimitReached =
  typedListener(EVENT.CHAT_AUTO_CONTINUATION_LIMIT_REACHED);

/** Spec 0021, Abschnitt 5 — s. `ChatAutoContinuationStartedEvent`-Doc-
 * Kommentar. */
export const onChatAutoContinuationStarted =
  typedListener(EVENT.CHAT_AUTO_CONTINUATION_STARTED);

/** Spec 0061, Abschnitt 4 — s. `AiBudgetWaitingEvent`-Doc-Kommentar. */
export const onAiBudgetWaiting = typedListener(EVENT.AI_BUDGET_WAITING);

/** Spec 0010 — bewusst app-weit abonniert (z. B. in `App.tsx`), nicht nur
 * innerhalb einer offenen `ChatPanel`-Instanz: der Vorschlag kann eintreffen,
 * nachdem der Nutzer den Session-Screen bereits verlassen hat. */
export const onNoteUpdateSuggested =
  typedListener(EVENT.NOTE_UPDATE_SUGGESTED);

/** Spec 0057, §4.2 (Etappe 4) — der ERSTE Dialog, bevor überhaupt ein
 * KI-Aufruf stattgefunden hat. App-weit, aus demselben Grund wie
 * `onNoteUpdateSuggested`. */
export const onNoteShrinkSuggested =
  typedListener(EVENT.NOTE_SHRINK_SUGGESTED);

/** Spec 0057, §4.2/§6 — der KI-Aufruf hinter "Ja, zusammenfassen" ist
 * fehlgeschlagen oder abgelaufen; die gespeicherte Notiz blieb unverändert. */
export const onNoteShrinkFailed = typedListener(EVENT.NOTE_SHRINK_FAILED);

/** Spec 0058 (Politur-Paket) — das Erfolgs-Gegenstück, s.
 * `NoteShrinkSucceededEvent`-Doc-Kommentar. */
export const onNoteShrinkSucceeded =
  typedListener(EVENT.NOTE_SHRINK_SUCCEEDED);

/** Spec 0012 — läuft, anders als `onChatActionProposed`, nie durch einen
 * Bestätigungsdialog (s. `ChatDocumentGeneratedEvent`-Doc-Kommentar). */
export const onChatDocumentGenerated =
  typedListener(EVENT.CHAT_DOCUMENT_GENERATED);

/** Issue #545: the AI asked a question with answer options (no UI yet). */
export const onChatQuestionAsked =
  typedListener(EVENT.CHAT_QUESTION_ASKED);

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser ---------------------

export const onSftpTransferStarted =
  typedListener(EVENT.SFTP_TRANSFER_STARTED);

export const onSftpTransferProgress =
  typedListener(EVENT.SFTP_TRANSFER_PROGRESS);

export const onSftpTransferFinished =
  typedListener(EVENT.SFTP_TRANSFER_FINISHED);

// --- Spec 0028: MCP-Server-Integration -----------------------------------

/** Spec 0028, Abschnitt 9a — signalisiert, für `serverId` einen Tab zu
 * öffnen/zu diesem zu wechseln, bevor ein Host-Key- oder Bestätigungsdialog
 * für diese Session erscheint. Nur für die vier aktionsauslösenden Tools. */
export const onMcpActionTabRequested =
  typedListener(EVENT.MCP_ACTION_TAB_REQUESTED);

// --- Spec 0101, Etappe 3: Startfragen im Fenster -------------------------

/** §2 Frage 3: eine Startfrage (D1–D4, zweite Bestätigung, neues
 * Master-Passwort, Hinweis) erscheint im Fenster.
 *
 * **Der Zuhörer muss stehen, bevor das erste Entsperr-Kommando läuft.**
 * `emit` im Backend liefert `Ok`, auch wenn niemand zuhört: Geht das
 * Ereignis verloren, läuft die Frage dort in die Zeitgrenze von fünf
 * Minuten (s. `crate::window_prompt::PROMPT_TIMEOUT`) und der Start bricht
 * mit „keine Antwort erhalten" ab. */
export const onStartupPrompt = typedListener(EVENT.STARTUP_PROMPT);

/** A16: Der Zustand steht jetzt — die Oberfläche darf die App zeigen. */
export const onStartupUnlocked = (handler: () => void): Promise<UnlistenFn> =>
  listenTyped(EVENT.STARTUP_UNLOCKED, () => handler());

/** Base64 → `Uint8Array`, für `TerminalOutputEvent.data` (s. `crate::events`). */
export function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/** Issue #1019: a pending confirmation was discarded by a reconnect or
 * expired; the chat card switches to a final state without buttons. */
export const onActionConfirmationSettled =
  typedListener(EVENT.ACTION_CONFIRMATION_SETTLED);
/** Issue #628: the backend timed out a pending note confirmation (update or
 * shrink suggestion); the card switches to its "expired" state. */
export const onNoteUpdateExpired =
  typedListener(EVENT.NOTE_UPDATE_EXPIRED);
