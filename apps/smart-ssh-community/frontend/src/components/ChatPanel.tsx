import { type FormEvent, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { acceptAndCreateRule, commandErrorCode, commandErrorMessage, continueTruncatedResponse, retryChatTurn, exportDocument, listAiProviders, respondToAction, sendChatMessage, stopAutoContinuation, startNewChat } from "../api";
import { useStickToBottom } from "../hooks/useStickToBottom";
import { Composer } from "./chat/Composer";
import { useChatComposer } from "./chat/useChatComposer";
import { useVoiceComposer } from "./chat/useVoiceComposer";
import { loadRiskClassifierSettings } from "../riskSettings";
import type { ActionUserDecision, DocumentFormat, SuggestionVerdictDto, SuggestionPatternDto, Scope } from "../types";
import { freshId } from "../chatResync";
import { Button } from "./ui";
import { deriveQuestionStates } from "./QuestionCard";
import { useChatTranscript } from "./chat/useChatTranscript";
import { ChatItemView } from "./chat/ChatItemView";

// Re-exported so existing importers (tests, `chatResync`) keep working.
export { ChatItemView } from "./chat/ChatItemView";
export type { ChatItem } from "./chat/chatItem";
export { QuickRuleButton } from "./chat/QuickRuleButton";
export { splitInjectionReason } from "./chat/chatItem";

interface ChatPanelProps {
  sessionId: string;
  /** Spec 0011, Abschnitt 4: Default-Scope für den Regel-Schnellvorschlag
   * ist der aktuell verbundene Server. */
  serverId: string;
  /** Spec 0017, Abschnitt 5: informiert die Tab-Leiste (`useSessionTabs`),
   * sobald der Nutzer eine wartende Bestätigung auflöst — auch bei
   * Ablehnung, die (anders als Approve/EditThenApprove) kein
   * `chat-action-result`-Event auslöst und sonst den Hinweis-Indikator auf
   * dem Tab hängen ließe. */
  onActionSettled: (sessionId: string) => void;
  /** Spec 0104: gesetzt für eine MCP-Sitzung — statt der Eingabezeile
   * erscheint dieser Hinweis. Die Aktionskarten (Bestätigen/Ablehnen) und
   * ihre Ergebnisse bleiben unverändert. */
  readOnlyHint?: string;
}

export function ChatPanel({ sessionId, serverId, onActionSettled, readOnlyHint }: ChatPanelProps) {
  const { t } = useTranslation();
  const chatAreaRef = useRef<HTMLDivElement>(null);
  // Spec 0055, Teil 1: `<textarea>` (Shift+Enter / auto-grow).
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const composer = useChatComposer(sessionId, serverId, inputRef, chatAreaRef);
  const { hasDropHandler, dropOver } = composer;
  const [sending, setSending] = useState(false);
  /** Spec 0066, §2: Anzahl noch nicht aufgelöster `sendChatMessage`/
   * `continueTruncatedResponse`-Aufrufe. Eingereihte Aufrufe lösen sofort
   * auf; ein Aufruf, der im Backend selbst einen Turn startet, erst am Ende
   * — `sending` ist genau dann `false`, wenn im Backend nichts mehr läuft. */
  const pendingSendsRef = useRef(0);
  /** Spec 0021, Abschnitt 5: `true`, sobald eine *automatische* Folgerunde
   * läuft (KI reagiert auf ein Aktionsergebnis, ohne dass der Nutzer
   * getippt hat) — steuert den "Automatik läuft"-Indikator samt
   * "Automatik stoppen"-Button. Wird zusammen mit `sending` zurückgesetzt
   * (`handleSubmit`s `finally`): `send_chat_message` läuft synchron über
   * die komplette Runden-Kette, dessen Promise-Auflösung ist also das
   * zuverlässige "die ganze Kette ist zu Ende"-Signal, unabhängig davon, ob
   * sie regulär endete, am Sicherheits-Cap oder durch "Automatik stoppen". */
  /** Issue #271: „Neuer Chat" läuft (Doppelklick-Schutz) bzw. ist gescheitert. */
  const [startingNewChat, setStartingNewChat] = useState(false);
  const [newChatError, setNewChatError] = useState<string | null>(null);
  const [hasActiveProvider, setHasActiveProvider] = useState<boolean | null>(null);
  /** Spec 0026, Abschnitt 4: nur bei aktivierter Zweitmeinung überhaupt
   * einen Lade-Indikator zeigen — sonst käme (bis ein `risk-assessment-
   * updated`-Event ausbliebe) fälschlich ein Spinner, der nie verschwindet.
   * Kann vom tatsächlichen Session-Verhalten abweichen, falls die
   * Einstellung erst NACH dem `connect()` dieser Session geändert wurde
   * (der Backend-Provider wird einmalig bei `connect()` aufgelöst, s.
   * `Session::risk_second_opinion_provider`-Doc-Kommentar) — ein bewusst
   * akzeptierter kleiner Randfall, kein Korrektheitsproblem. */
  const [riskSecondOpinionEnabled, setRiskSecondOpinionEnabled] = useState(false);
  const { state: transcript, dispatch } = useChatTranscript(sessionId, {
    riskSecondOpinionEnabled,
    onActionSettled,
  });
  const { items, contextUsage, retrying, retryNow, aiWait, waitNow, autoContinuing, liveOutputs, injectionChecks, waitingActions } = transcript;
  const scrollRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const {
    atBottom,
    onScroll: handleChatScroll,
    scrollToBottom,
  } = useStickToBottom(scrollRef, contentRef, items);
  /** Issue #325: live output per running action, kept outside `items` so
   * that its updates (up to ten per second) do not trigger the chat's
   * jump-to-the-end effect below. Removed when the result arrives. */
  // Issue #693: injection check state per checked item (action / web card id).
  // Issue #327: actions whose command is probably waiting for input.
  // Spec 0055, Teil 1: `<textarea>` statt `<input type="text">` für die
  // mehrzeilige Eingabe (Shift+Enter/Auto-Grow) — dieselbe DOM-API
  // (`.value`/`.selectionStart`/`.setSelectionRange`), die restliche
  // Historien-Navigationslogik unten bleibt unverändert.
  const jumpButtonRef = useRef<HTMLButtonElement>(null);
  // Issue #758: the jump button hides once the column is at the bottom. A
  // hidden element drops focus to <body>, so hand focus to the chat input
  // first when the button still has it.
  useLayoutEffect(() => {
    if (atBottom && jumpButtonRef.current && document.activeElement === jumpButtonRef.current) {
      inputRef.current?.focus();
    }
  }, [atBottom]);
  // Spec 0108: voice input; recognised text goes into the draft.
  const voice = useVoiceComposer(composer.draft, composer.setDraft, inputRef);

  useEffect(() => {
    listAiProviders()
      .then((providers) => setHasActiveProvider(providers.some((p) => p.isActive)))
      .catch(() => setHasActiveProvider(null));
  }, []);

  useEffect(() => {
    loadRiskClassifierSettings()
      .then((settings) => setRiskSecondOpinionEnabled(settings.enabled))
      .catch(() => setRiskSecondOpinionEnabled(false));
  }, []);



  const respond = (actionId: string, decision: ActionUserDecision) => {
    // Spec 0027: die tatsächliche Ausführung beginnt jetzt (nicht bei
    // "deny", das führt nie etwas aus) — Startzeitpunkt für den
    // Lauf-Indikator.
    const startedAt =
      decision.decision === "approve" || decision.decision === "editThenApprove"
        ? Date.now()
        : null;
    dispatch({ type: "actionResponded", actionId, decision: decision.decision, startedAt });
    onActionSettled(sessionId);
    respondToAction(sessionId, actionId, decision).catch((err) => {
      // Issue #1019: the optimistic state is rolled back. An unknown
      // confirmation was settled by the backend: the card shows "expired",
      // never "approved". Any other error leaves the request open.
      const unknown = commandErrorCode(err) === "ACTION_CONFIRMATION_UNKNOWN";
      dispatch({
        type: "actionRespondFailed",
        actionId,
        unknown,
        errorItem: { type: "error", id: freshId(), message: commandErrorMessage(err), code: null },
      });
    });
  };

  /** Spec 0021, Abschnitt 5 / Spec 0066, §1: bricht den laufenden
   * KI-Request sofort ab und verhindert weitere automatische Runden. Ein
   * offener Bestätigungsdialog bleibt stehen, ein laufendes Kommando läuft
   * zu Ende. */
  const handleStopAutoContinuation = () => {
    dispatch({ type: "autoContinuationStopped" });
    stopAutoContinuation(sessionId).catch((err) =>
      dispatch({ type: "itemAppended", item: { type: "error", id: freshId(), message: commandErrorMessage(err), code: null } }),
    );
  };

  /** Spec 0011, Abschnitt 3: `accept_and_create_rule` legt die Regel an
   * **und** löst die Confirm-Entscheidung selbst auf (Backend) — anders als
   * `respond()` oben ruft dies also `respondToAction` nicht zusätzlich auf.
   *
   * `editedCommand`: unabhängiger Review-Pass (Spec 0007/0008/0011) —
   * `null`, falls der Text im Bearbeiten-Feld unverändert dem
   * KI-Vorschlag entspricht (Backend löst dann wie zuvor über `Approve`
   * auf); sonst der bearbeitete Text, damit das Backend über
   * `EditThenApprove` tatsächlich DIESEN Text erneut durch die
   * Filter-Engine schickt und ausführt — sonst würde bei jedem editierten
   * Kommando das ursprüngliche, unbearbeitete Kommando ausgeführt, während
   * die neue Regel aus dem bearbeiteten Text abgeleitet wird (genau der
   * Bestätigungsdialog-Bypass, gegen den `EditThenApprove` beim normalen
   * "Ausführen"-Button bereits schützt, s. `handleApprove` oben in
   * `ConfirmActionForm`).
   * `startedAt`: wie bei `respond()` oben — dieser Pfad führt immer aus
   * (nie "deny"), ohne das würde weder der Lauf-Indikator noch der
   * Abbrechen-Button (Spec 0027) für ein per Schnellregel akzeptiertes
   * Kommando erscheinen. */
  const acceptWithRule = (
    actionId: string,
    patterns: SuggestionPatternDto[],
    scope: Scope,
    editedCommand: string | null,
    verdict?: SuggestionVerdictDto,
  ) => {
    dispatch({ type: "ruleAccepting", actionId, startedAt: Date.now() });
    onActionSettled(sessionId);
    acceptAndCreateRule(
      sessionId,
      actionId,
      patterns,
      scope,
      undefined,
      editedCommand,
    )
      .then(() => {
        // Issue #596: no silent failure — repeat why the command still asks.
        if (verdict && !verdict.autoExec) {
          dispatch({ type: "itemAppended", item: {
              type: "ruleNotice",
              id: freshId(),
              reasons: verdict.reasons.map((r) => r.code),
            } });
        }
      })
      .catch((err) =>
        dispatch({
          type: "itemAppended",
          // Spec 0077, 3.1.4: Code weiterreichen statt `null`, damit ein
        // ungültiges Muster auch hier übersetzt erscheint. Die Anzeige
        // ersetzt die `message` durch den übersetzten Satz — hier bewusst
        // ohne den Fehlertext der Bibliothek: Es wird ein Vorschlag
        // angelegt, kein selbstgeschriebenes Muster.
          item: { type: "error", id: freshId(), message: commandErrorMessage(err), code: commandErrorCode(err) },
        }),
      );
  };

  /** Spec 0012, Abschnitt 3: der native Speichern-Dialog selbst lebt im
   * Backend (`export_document`) — bricht der Nutzer ihn ab, kehrt der
   * Command einfach ohne Fehler zurück (s. dortiger Doc-Kommentar), hier
   * ist dann schlicht nichts zu tun. */
  const handleExport = async (contentMarkdown: string, title: string, format: DocumentFormat) => {
    try {
      await exportDocument(contentMarkdown, title, format);
    } catch (err) {
      dispatch({ type: "itemAppended", item: { type: "error", id: freshId(), message: commandErrorMessage(err), code: null } });
      throw err;
    }
  };

  /** Spec 0065, Teil 2: „Weiter"-Knopf an einer abgeschnittenen Antwort —
   * läuft durch `continueTruncatedResponse` (identischer Backend-Pfad wie
   * `sendChatMessage`, s. dortiger Kommentar). Entfernt das `truncated`-Flag
   * von der Nachricht sofort optimistisch, damit der Knopf nicht doppelt
   * klickbar bleibt, während die Anfrage läuft. */
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

  const runSend = async (call: () => Promise<void>) => {
    pendingSendsRef.current += 1;
    // Issue #694: drop a stale wait line from the previous turn.
    if (pendingSendsRef.current === 1) dispatch({ type: "turnStarted" });
    setSending(true);
    try {
      await call();
    } catch (err) {
      dispatch({ type: "itemAppended", item: { type: "error", id: freshId(), message: commandErrorMessage(err), code: null } });
    } finally {
      // Spec 0021, Abschnitt 7: Fail-Safe — läuft in JEDEM Fall (Erfolg,
      // Fehler, gleich welcher der vier Ausgänge zuletzt griff), damit die
      // Eingabe nie dauerhaft gesperrt bleiben kann, selbst wenn die
      // automatische Fortsetzung selbst fehlschlägt (z. B. Netzwerkfehler
      // beim Folge-Request an den KI-Provider — landet als `chat-error`,
      // aber `sendChatMessage()` löst trotzdem auf, s. `run_chat_turn`).
      pendingSendsRef.current -= 1;
      if (pendingSendsRef.current === 0) {
        setSending(false);
        // Läuft im Backend nichts mehr, wartet auch keine Nachricht mehr.
        dispatch({ type: "turnSettled" });
      }
    }
  };

  /** Issue #547: an option click is a normal user message through the
   * existing send path — never an action response. */
  const handleQuestionAnswer = async (text: string) => {
    const queued = sending;
    scrollToBottom();
    dispatch({ type: "itemAppended", item: { type: "user", id: freshId(), text, ...(queued ? { queued: true } : {}) } });
    await runSend(() => sendChatMessage(sessionId, text));
  };

  const questionStates = deriveQuestionStates(items);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const { draft, attachments, setDraft, setAttachments, setComposerError } = composer;
    const text = draft.trim();
    if (!text || voice.voicePhase !== "idle") return;
    const sentAttachments = attachments;
    setComposerError(null);
    // Spec 0066, §2: läuft schon eine Antwort, reiht das Backend die
    // Nachricht ein (kehrt sofort zurück) und schickt sie mit der nächsten
    // Anfrage an die KI — die laufende Antwort läuft weiter.
    const queued = sending;
    const itemId = freshId();
    // Issue #540: an own message always jumps to the bottom and follows again.
    scrollToBottom();
    dispatch({ type: "itemAppended", item: {
        type: "user",
        id: itemId,
        text,
        ...(queued ? { queued: true } : {}),
        ...(sentAttachments.length > 0
          ? { attachments: sentAttachments.map((a) => ({ name: a.name, kind: a.kind })) }
          : {}),
      } });
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
        // Spec 0015, Abschnitt 5: das Backend hat den Prompt gespeichert (samt
        // Dedup und 200er-Grenze) — Liste neu laden, damit er sofort per
        // Pfeiltaste erreichbar ist. Beim lokalen Pseudo-Server bleibt sie leer.
        composer.reloadPromptHistory();
      } catch (err) {
        // Issue #315: „Neuer Chat" tauscht den Chat gerade aus; das Backend hat
        // die Nachricht abgelehnt. Sie verschwindet aus dem Verlauf und bleibt
        // im Eingabefeld (ein inzwischen getippter Entwurf bleibt unberührt).
        if (commandErrorCode(err) === "CHAT_SWITCHING") {
          dispatch({ type: "itemRemoved", id: itemId });
          setDraft((current) => (current === "" ? text : current));
        }
        // Issue #471: lehnt das Backend eine Nachricht mit Anhängen ab
        // (Grenze, Modell, gesperrte Funktion), wurde nichts gesendet: Text
        // und Chips bleiben zum Korrigieren, der Fehler steht am Eingabefeld.
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
  const confirmationOpen = items.some(
    (it) =>
      it.type === "action" &&
      !it.responded &&
      typeof it.decision === "object" &&
      "Confirm" in it.decision,
  );
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

  const voiceLocked = voice.voiceLocked;

  return (
    <div ref={chatAreaRef} className="relative flex h-full flex-col">
      {hasDropHandler && dropOver && (
        <div
          data-testid="chat-drop-highlight"
          className="pointer-events-none absolute inset-0 z-10 border-2 border-dashed border-accent-strong bg-accent-subtle/30"
        />
      )}
      {readOnlyHint === undefined && (
        <div className="flex items-center justify-end gap-2 border-b border-border px-3 py-1.5">
          {newChatError !== null && (
            <span role="alert" className="flex-1 text-xs text-danger-fg-soft">
              {newChatError}
            </span>
          )}
          <Button
            size="sm"
            onClick={handleNewChat}
            disabled={newChatDisabled}
            title={newChatDisabled ? t("chat.newChatDisabledHint") : undefined}
          >
            {t("chat.newChat")}
          </Button>
        </div>
      )}
      <div className="relative flex min-h-0 flex-1 flex-col">
      <div
        ref={scrollRef}
        onScroll={handleChatScroll}
        className="flex-1 overflow-y-auto p-4"
      >
      <div ref={contentRef} className="space-y-3">
        {items.map((item) => (
          <ChatItemView
            key={item.id}
            item={item}
            liveOutput={item.type === "action" ? liveOutputs[item.actionId] : undefined}
            waitingForInput={item.type === "action" && item.actionId in waitingActions}
            injectionCheck={
              item.type === "action"
                ? injectionChecks[item.actionId]
                : item.type === "webActivity" && item.activityId
                  ? injectionChecks[item.activityId]
                  : undefined
            }
            onRespond={respond}
            onAcceptWithRule={acceptWithRule}
            onExport={handleExport}
            onContinueTruncated={handleContinueTruncated}
            onRetryTurn={handleRetryTurn}
            retryDisabled={sending}
            questionState={questionStates.get(item.id)}
            onQuestionAnswer={handleQuestionAnswer}
            serverId={serverId}
            sessionId={sessionId}
          />
        ))}
        {/* Spec 0021, Abschnitt 5: "Automatik läuft"-Indikator mit
         * Stopp-Möglichkeit hat Vorrang vor dem generischen
         * "generiert Antwort"-Hinweis — sobald eine automatische Folgerunde
         * läuft, ist das die genauere, handlungsfähigere Information. */}
        {retrying ? (
          <output
            data-testid="chat-ai-retrying"
            className="flex items-center gap-2 rounded-lg border border-warning-border-strong/50 bg-warning-subtle/30 px-3 py-2 text-xs text-warning-fg-softer"
          >
            <span className="flex-1">
              {t("chat.aiRetrying", {
                seconds: Math.max(0, Math.ceil((retrying.deadline - retryNow) / 1000)),
                attempt: retrying.attempt + 1,
                max: retrying.max + 1,
              })}
            </span>
            <Button size="sm" onClick={handleStopAutoContinuation}>
              {t("chat.stop")}
            </Button>
          </output>
        ) : aiWait &&
          (sending || autoContinuing) &&
          (!aiWait.gotContent || waitNow - aiWait.since > 10_000) ? (
          <output
            data-testid="chat-ai-waiting"
            className="flex items-center gap-2 rounded-lg border border-accent-border-strong/50 bg-accent-subtle/40 px-3 py-2 text-xs text-accent-fg-soft"
          >
            <span className="inline-block h-2 w-2 animate-ping rounded-full bg-accent-fg" />
            <span className="flex-1">
              {t("chat.aiWaiting", {
                provider: aiWait.provider,
                seconds: Math.max(0, Math.floor((waitNow - aiWait.since) / 1000)),
              })}
            </span>
            <Button size="sm" onClick={handleStopAutoContinuation}>
              {t("chat.stop")}
            </Button>
          </output>
        ) : autoContinuing ? (
          <div className="flex items-center gap-2 rounded-lg border border-accent-border-strong/50 bg-accent-subtle/40 px-3 py-2 text-xs text-accent-fg-soft">
            <span className="inline-block h-2 w-2 animate-ping rounded-full bg-accent-fg" />
            <span className="flex-1">
              🤖 {t("chat.autoContinuing")}
            </span>
            <Button size="sm" onClick={handleStopAutoContinuation}>
              {t("chat.stopAutoContinuation")}
            </Button>
          </div>
        ) : (
          sending && (
            <div className="flex items-center gap-2 rounded-lg bg-surface-raised/80 px-3 py-2 text-xs text-accent-fg-soft">
              <span className="inline-block h-2 w-2 animate-ping rounded-full bg-accent-fg" />
              <span className="flex-1">{t("chat.generating")}</span>
              {/* Spec 0066, §1: bricht den laufenden KI-Request sofort ab. */}
              <Button size="sm" onClick={handleStopAutoContinuation}>
                {t("chat.stop")}
              </Button>
            </div>
          )
        )}
      </div>
      </div>
      {/* Always mounted: the late scroll event that flips `atBottom` must never
          unmount a focused button (focus would fall back to <body>). While at
          the bottom it is invisible, aria-hidden and out of the Tab order. */}
      {(
        <button
          type="button"
          ref={jumpButtonRef}
          onClick={() => {
            scrollToBottom();
            inputRef.current?.focus();
          }}
          aria-hidden={atBottom ? true : undefined}
          tabIndex={atBottom ? -1 : undefined}
          aria-label={t("chat.jumpToLatest")}
          title={t("chat.jumpToLatest")}
          data-testid="chat-jump-to-latest"
          className={`absolute bottom-3 left-1/2 flex h-9 w-9 -translate-x-1/2 items-center justify-center rounded-full border border-border-strong bg-surface-raised text-fg shadow-lg hover:bg-border focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent-fg${atBottom ? " invisible" : ""}`}
        >
          <svg aria-hidden="true" viewBox="0 0 20 20" className="h-5 w-5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M10 4v12M5 11l5 5 5-5" />
          </svg>
        </button>
      )}
      </div>

      {readOnlyHint !== undefined ? (
        <div className="border-t border-border bg-surface-raised p-3 text-xs text-fg-subtle">
          {readOnlyHint}
        </div>
      ) : hasActiveProvider === false ? (
        <div className="border-t border-border p-4 text-sm text-warning-fg-soft">
          {t("chat.noActiveProvider")}
        </div>
      ) : (
        <Composer
          draft={composer.draft}
          attachments={composer.attachments}
          composerError={composer.composerError}
          composerCtx={composer.composerCtx}
          composerActions={composer.composerActions}
          invokeComposerAction={composer.invokeComposerAction}
          contextUsage={contextUsage}
          inputRef={inputRef}
          chatAreaRef={chatAreaRef}
          onSubmit={handleSubmit}
          onDraftChange={composer.handleDraftChange}
          onKeyDown={composer.handleInputKeyDown}
          voiceEnabled={voice.voiceEnabled}
          voiceLocked={voiceLocked}
          voiceHandlers={voice.voiceHandlers}
          onVoicePhaseChange={voice.setVoicePhase}
        />
      )}
    </div>
  );
}
