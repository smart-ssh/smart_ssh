import { type Dispatch, type FormEvent, useRef, useState } from "react";
import {
  commandErrorCode,
  commandErrorMessage,
  continueTruncatedResponse,
  retryChatTurn,
  sendChatMessage,
  startNewChat,
  stopAutoContinuation,
} from "../../api";
import { freshId } from "../../chatResync";
import type { ChatTranscriptAction } from "./chatTranscriptReducer";
import type { useChatComposer } from "./useChatComposer";

export interface UseChatSendOptions {
  composer: ReturnType<typeof useChatComposer>;
  dispatch: Dispatch<ChatTranscriptAction>;
  /** Voice input is active: submitting is ignored. */
  voiceLocked: boolean;
  scrollToBottom: () => void;
  /** An automatic follow-up round is running (blocks "new chat"). */
  autoContinuing: boolean;
  /** A confirmation is waiting for the user (blocks "new chat"). */
  confirmationOpen: boolean;
}

/**
 * Sending a chat turn: submit, retry, continue a truncated answer, answer a
 * question, stop the automatic continuation and start a new chat. Exposes the
 * busy state the composer and the transcript need.
 */
export function useChatSend(sessionId: string, options: UseChatSendOptions) {
  const { composer, dispatch, voiceLocked, scrollToBottom, autoContinuing, confirmationOpen } = options;
  const [sending, setSending] = useState(false);
  /** Spec 0066, §2: Anzahl noch nicht aufgelöster `sendChatMessage`/
   * `continueTruncatedResponse`-Aufrufe. Eingereihte Aufrufe lösen sofort
   * auf; ein Aufruf, der im Backend selbst einen Turn startet, erst am Ende
   * — `sending` ist genau dann `false`, wenn im Backend nichts mehr läuft. */
  const pendingSendsRef = useRef(0);
  /** Issue #271: „Neuer Chat" läuft (Doppelklick-Schutz) bzw. ist gescheitert. */
  const [startingNewChat, setStartingNewChat] = useState(false);
  const [newChatError, setNewChatError] = useState<string | null>(null);

  const pushError = (err: unknown) =>
    dispatch({ type: "itemAppended", item: { type: "error", id: freshId(), message: commandErrorMessage(err), code: null } });

  /** Spec 0021, Abschnitt 5 / Spec 0066, §1: bricht den laufenden
   * KI-Request sofort ab und verhindert weitere automatische Runden. Ein
   * offener Bestätigungsdialog bleibt stehen, ein laufendes Kommando läuft
   * zu Ende. */
  const handleStopAutoContinuation = () => {
    dispatch({ type: "autoContinuationStopped" });
    stopAutoContinuation(sessionId).catch(pushError);
  };

  const runSend = async (call: () => Promise<void>) => {
    pendingSendsRef.current += 1;
    // Issue #694: drop a stale wait line from the previous turn.
    if (pendingSendsRef.current === 1) dispatch({ type: "turnStarted" });
    setSending(true);
    try {
      await call();
    } catch (err) {
      pushError(err);
    } finally {
      // Spec 0021, Abschnitt 7: Fail-Safe — läuft in JEDEM Fall, damit die
      // Eingabe nie dauerhaft gesperrt bleiben kann, selbst wenn die
      // automatische Fortsetzung selbst fehlschlägt.
      pendingSendsRef.current -= 1;
      if (pendingSendsRef.current === 0) {
        setSending(false);
        // Läuft im Backend nichts mehr, wartet auch keine Nachricht mehr.
        dispatch({ type: "turnSettled" });
      }
    }
  };

  /** Spec 0065, Teil 2: „Weiter"-Knopf an einer abgeschnittenen Antwort.
   * Entfernt das `truncated`-Flag sofort optimistisch, damit der Knopf nicht
   * doppelt klickbar bleibt, während die Anfrage läuft. */
  const handleContinueTruncated = async (itemId: string) => {
    // Spec 0066, spec-reviewer-Fund: während eine Antwort läuft, würde die
    // Fortsetzungs-Anweisung eingereiht und in einem fremden Turn landen.
    if (sending) return;
    dispatch({ type: "truncatedCleared", id: itemId });
    await runSend(() => continueTruncatedResponse(sessionId));
  };

  /** Issue #600: "Try again" on a retryable error card. */
  const handleRetryTurn = async (itemId: string) => {
    if (sending) return;
    dispatch({ type: "itemRemoved", id: itemId });
    await runSend(() => retryChatTurn(sessionId));
  };

  /** Issue #547: an option click is a normal user message through the
   * existing send path — never an action response. */
  const handleQuestionAnswer = async (text: string) => {
    const queued = sending;
    scrollToBottom();
    dispatch({ type: "itemAppended", item: { type: "user", id: freshId(), text, ...(queued ? { queued: true } : {}) } });
    await runSend(() => sendChatMessage(sessionId, text));
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const { draft, attachments, setDraft, setAttachments, setComposerError } = composer;
    const text = draft.trim();
    if (!text || voiceLocked) return;
    const sentAttachments = attachments;
    setComposerError(null);
    // Spec 0066, §2: läuft schon eine Antwort, reiht das Backend die
    // Nachricht ein (kehrt sofort zurück) und schickt sie mit der nächsten
    // Anfrage an die KI — die laufende Antwort läuft weiter.
    const queued = sending;
    const itemId = freshId();
    // Issue #540: an own message always jumps to the bottom and follows again.
    scrollToBottom();
    dispatch({
      type: "itemAppended",
      item: {
        type: "user",
        id: itemId,
        text,
        ...(queued ? { queued: true } : {}),
        ...(sentAttachments.length > 0
          ? { attachments: sentAttachments.map((a) => ({ name: a.name, kind: a.kind })) }
          : {}),
      },
    });
    setDraft("");
    setAttachments([]);
    composer.resetHistoryNav();
    await runSend(async () => {
      try {
        if (sentAttachments.length > 0) {
          await sendChatMessage(
            sessionId,
            text,
            sentAttachments.map((a) => a.id),
          );
        } else {
          await sendChatMessage(sessionId, text);
        }
        // Spec 0015, Abschnitt 5: das Backend hat den Prompt gespeichert —
        // Liste neu laden, damit er sofort per Pfeiltaste erreichbar ist.
        composer.reloadPromptHistory();
      } catch (err) {
        // Issue #315: „Neuer Chat" tauscht den Chat gerade aus; das Backend hat
        // die Nachricht abgelehnt. Sie verschwindet aus dem Verlauf und bleibt
        // im Eingabefeld (ein inzwischen getippter Entwurf bleibt unberührt).
        if (commandErrorCode(err) === "CHAT_SWITCHING") {
          dispatch({ type: "itemRemoved", id: itemId });
          setDraft((current) => (current === "" ? text : current));
        }
        // Issue #471: lehnt das Backend eine Nachricht mit Anhängen ab, wurde
        // nichts gesendet: Text und Chips bleiben zum Korrigieren, der Fehler
        // steht am Eingabefeld.
        if (sentAttachments.length > 0) {
          dispatch({ type: "itemRemoved", id: itemId });
          setDraft((current) => (current === "" ? text : current));
          setAttachments((current) => (current.length === 0 ? sentAttachments : current));
          setComposerError(commandErrorMessage(err));
          return;
        }
        throw err;
      }
    });
  };

  // Issue #271 (Entscheidung 3): gesperrt, solange ein Turn läuft oder eine
  // Bestätigung offen ist — das Backend prüft zusätzlich.
  const newChatDisabled = sending || autoContinuing || confirmationOpen || startingNewChat;

  const handleNewChat = async () => {
    if (newChatDisabled) return;
    setStartingNewChat(true);
    setNewChatError(null);
    try {
      await startNewChat(sessionId);
      // Issue #648: the new chat has no usage yet; keep the window size.
      dispatch({ type: "chatCleared" });
      composer.resetHistoryNav();
    } catch (err) {
      setNewChatError(commandErrorMessage(err));
    } finally {
      setStartingNewChat(false);
    }
  };

  return {
    sending,
    handleSubmit,
    handleContinueTruncated,
    handleRetryTurn,
    handleQuestionAnswer,
    handleStopAutoContinuation,
    handleNewChat,
    newChatDisabled,
    newChatError,
  };
}
