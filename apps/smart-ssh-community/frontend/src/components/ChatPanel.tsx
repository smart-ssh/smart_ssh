import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, exportDocument, listAiProviders } from "../api/api";
import { useStickToBottom } from "../hooks/useStickToBottom";
import { Composer } from "./chat/Composer";
import { useChatComposer } from "./chat/useChatComposer";
import { useVoiceComposer } from "./chat/useVoiceComposer";
import { loadRiskClassifierSettings } from "../settings/riskSettings";
import type { DocumentFormat } from "../api/types";
import { freshId } from "../lib/chatResync";
import { Button } from "./ui";
import { deriveQuestionStates, type QuestionState } from "./QuestionCard";
import { useChatTranscript } from "./chat/useChatTranscript";
import { ChatItemView } from "./chat/ChatItemView";
import { useStableCallback } from "./chat/useStableCallback";
import { useChatSend } from "./chat/useChatSend";
import { useActionDecisions } from "./chat/useActionDecisions";

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
  } = useStickToBottom(scrollRef, contentRef, items, () => {
    // Runs before the state change that hides the jump button (#758): once
    // the button is `invisible`, the browser may drop its focus to <body>
    // before any layout effect gets to hand it on.
    if (document.activeElement === jumpButtonRef.current) inputRef.current?.focus();
  });
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



  /** Spec 0012, Abschnitt 3: der native Speichern-Dialog selbst lebt im
   * Backend (`export_document`) — bricht der Nutzer ihn ab, kehrt der
   * Command einfach ohne Fehler zurück (s. dortiger Doc-Kommentar), hier
   * ist dann schlicht nichts zu tun. */
  const handleExport = useStableCallback(async (contentMarkdown: string, title: string, format: DocumentFormat) => {
    try {
      await exportDocument(contentMarkdown, title, format);
    } catch (err) {
      dispatch({ type: "itemAppended", item: { type: "error", id: freshId(), message: commandErrorMessage(err), code: null } });
      throw err;
    }
  });

  const confirmationOpen = items.some(
    (it) =>
      it.type === "action" &&
      !it.responded &&
      typeof it.decision === "object" &&
      "Confirm" in it.decision,
  );
  const {
    sending,
    handleSubmit,
    handleContinueTruncated: continueTruncatedRaw,
    handleRetryTurn: retryTurnRaw,
    handleQuestionAnswer,
    handleStopAutoContinuation,
    handleNewChat,
    newChatDisabled,
    newChatError,
  } = useChatSend(sessionId, {
    composer,
    dispatch,
    voiceLocked: voice.voiceLocked,
    scrollToBottom,
    autoContinuing,
    confirmationOpen,
  });

  const { respond: respondRaw, acceptWithRule: acceptWithRuleRaw } = useActionDecisions(
    sessionId,
    dispatch,
    onActionSettled,
  );
  // Stable identities for the memoised item views.
  const respond = useStableCallback(respondRaw);
  const acceptWithRule = useStableCallback(acceptWithRuleRaw);
  const handleContinueTruncated = useStableCallback(continueTruncatedRaw);
  const handleRetryTurn = useStableCallback(retryTurnRaw);
  const handleQuestionAnswerStable = useStableCallback(handleQuestionAnswer);
  // Keep the previous state object for a question whose state did not change.
  const questionStatesRef = useRef(new Map<string, QuestionState>());
  const questionStates = useMemo(() => {
    const prev = questionStatesRef.current;
    const next = deriveQuestionStates(items);
    for (const [id, st] of next) {
      const old = prev.get(id);
      if (
        old &&
        old.locked === st.locked &&
        old.ownWords === st.ownWords &&
        old.selected.length === st.selected.length &&
        old.selected.every((v, i) => v === st.selected[i])
      ) {
        next.set(id, old);
      }
    }
    questionStatesRef.current = next;
    return next;
  }, [items]);

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
            onQuestionAnswer={handleQuestionAnswerStable}
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
        <Button
          variant="bare"
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
        </Button>
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
