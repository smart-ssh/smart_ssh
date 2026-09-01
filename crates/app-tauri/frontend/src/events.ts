import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ChatActionProposedEvent,
  ChatActionResultEvent,
  ChatDocumentGeneratedEvent,
  ChatErrorEvent,
  ChatTextDeltaEvent,
  ConnectionStatusChangedEvent,
  HostKeyVerificationNeededEvent,
  NoteUpdateSuggestedEvent,
  SftpTransferFinishedEvent,
  SftpTransferStartedEvent,
  TerminalOutputEvent,
} from "./types";

// Dünne, typisierte Wrapper um `@tauri-apps/api/event`'s `listen()` — ein
// Name pro Event aus Spec 0007 Abschnitt 5, damit Aufrufer nie den
// String-Namen von Hand tippen (Tippfehlerquelle) und die Payload-Form aus
// `types.ts` automatisch mitbekommen.

export const onConnectionStatusChanged = (
  handler: (event: ConnectionStatusChangedEvent) => void,
): Promise<UnlistenFn> =>
  listen<ConnectionStatusChangedEvent>("connection-status-changed", (e) => handler(e.payload));

export const onHostKeyVerificationNeeded = (
  handler: (event: HostKeyVerificationNeededEvent) => void,
): Promise<UnlistenFn> =>
  listen<HostKeyVerificationNeededEvent>("host-key-verification-needed", (e) =>
    handler(e.payload),
  );

export const onTerminalOutput = (
  handler: (event: TerminalOutputEvent) => void,
): Promise<UnlistenFn> => listen<TerminalOutputEvent>("terminal-output", (e) => handler(e.payload));

export const onChatTextDelta = (
  handler: (event: ChatTextDeltaEvent) => void,
): Promise<UnlistenFn> => listen<ChatTextDeltaEvent>("chat-text-delta", (e) => handler(e.payload));

export const onChatActionProposed = (
  handler: (event: ChatActionProposedEvent) => void,
): Promise<UnlistenFn> =>
  listen<ChatActionProposedEvent>("chat-action-proposed", (e) => handler(e.payload));

export const onChatActionResult = (
  handler: (event: ChatActionResultEvent) => void,
): Promise<UnlistenFn> =>
  listen<ChatActionResultEvent>("chat-action-result", (e) => handler(e.payload));

export const onChatError = (handler: (event: ChatErrorEvent) => void): Promise<UnlistenFn> =>
  listen<ChatErrorEvent>("chat-error", (e) => handler(e.payload));

/** Spec 0010 — bewusst app-weit abonniert (z. B. in `App.tsx`), nicht nur
 * innerhalb einer offenen `ChatPanel`-Instanz: der Vorschlag kann eintreffen,
 * nachdem der Nutzer den Session-Screen bereits verlassen hat. */
export const onNoteUpdateSuggested = (
  handler: (event: NoteUpdateSuggestedEvent) => void,
): Promise<UnlistenFn> =>
  listen<NoteUpdateSuggestedEvent>("note-update-suggested", (e) => handler(e.payload));

/** Spec 0012 — läuft, anders als `onChatActionProposed`, nie durch einen
 * Bestätigungsdialog (s. `ChatDocumentGeneratedEvent`-Doc-Kommentar). */
export const onChatDocumentGenerated = (
  handler: (event: ChatDocumentGeneratedEvent) => void,
): Promise<UnlistenFn> =>
  listen<ChatDocumentGeneratedEvent>("chat-document-generated", (e) => handler(e.payload));

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser ---------------------

export const onSftpTransferStarted = (
  handler: (event: SftpTransferStartedEvent) => void,
): Promise<UnlistenFn> =>
  listen<SftpTransferStartedEvent>("sftp-transfer-started", (e) => handler(e.payload));

export const onSftpTransferFinished = (
  handler: (event: SftpTransferFinishedEvent) => void,
): Promise<UnlistenFn> =>
  listen<SftpTransferFinishedEvent>("sftp-transfer-finished", (e) => handler(e.payload));

/** Base64 → `Uint8Array`, für `TerminalOutputEvent.data` (s. `crate::events`). */
export function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}
