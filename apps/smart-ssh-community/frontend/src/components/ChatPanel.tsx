import { ContextUsageIndicator } from "./ContextUsageIndicator";
import { type FormEvent, type KeyboardEvent, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { acceptAndCreateRule, commandErrorCode, commandErrorMessage, continueTruncatedResponse, retryChatTurn, exportDocument, getChatHistory, getChatResync, listAiProviders, listPromptHistory, respondToAction, sendChatMessage, stopAutoContinuation, startNewChat } from "../api";
import { onActionDecisionEscalated, onAiBudgetWaiting, onChatActionProposed, onChatActionOutput, onChatActionWaitingInput, onChatActionResult, onActionConfirmationSettled, onChatAutoContinuationLimitReached, onChatAutoContinuationStarted, onChatDocumentGenerated, onChatQuestionAsked, onChatError, onChatAiRetrying, onChatAiRequestStarted, onChatQueuedMessagesSent, onChatResponseCancelled, onChatResponseEmpty, onChatResponseRefused, onChatWebResearchUnavailable, onChatResponseTruncated, onChatContextUsage, onChatContextCompacted, onChatTextDelta, onChatWebActivity, onChatInjectionCheck, onRiskAssessmentUpdated } from "../events";
import { isRetryableAiError } from "../errorCodes";
import { appendToDraft, loadVoiceInputEnabled, subscribeVoiceInputEnabled } from "../voiceInputSettings";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { useStickToBottom } from "../hooks/useStickToBottom";
import type { VoiceHandlers, VoicePhase } from "../useVoiceInput";
import { VoiceInputControl } from "./VoiceInputControl";
import { appendLiveOutput, type LiveOutput } from "../liveOutput";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { type ComposerAction, type ComposerAttachment, type ComposerContext, listComposerActions, listComposerDropHandlers, subscribeComposer } from "../extensions/registry";
import { isPointerInside } from "../dropZone";
import { formatBytes } from "../format";
import { initialHistoryNavState, navigateHistory, type HistoryNavState } from "../promptHistoryNav";
import { loadRiskClassifierSettings } from "../riskSettings";
import type { ActionUserDecision, DocumentFormat, SuggestionVerdictDto, SuggestionPatternDto, Scope, InjectionCheckState } from "../types";
import { actionItemFromProposed, freshId, historyEntryToItem, mergeResyncedItems } from "../chatResync";
import { Button, ErrorBoundary } from "./ui";
import { deriveQuestionStates } from "./QuestionCard";
import { ChatItemView } from "./chat/ChatItemView";
import { type ChatItem } from "./chat/chatItem";

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
  // The settled-event listener lives in an effect that must not resubscribe per render.
  const onActionSettledRef = useRef(onActionSettled);
  useEffect(() => {
    onActionSettledRef.current = onActionSettled;
  });
  const { t } = useTranslation();
  const [items, setItems] = useState<ChatItem[]>([]);
  // Issue #601: last real input size reported by the provider (numbers only).
  const [contextUsage, setContextUsage] = useState<{
    inputTokens: number;
    windowTokens: number;
  } | null>(null);
  const [draft, setDraft] = useState("");
  // Issue #471: Composer-Erweiterungspunkt, ausstehende Anhänge, Fehler am Eingabefeld.
  const composerActions = useSyncExternalStore(subscribeComposer, listComposerActions);
  const dropHandlers = useSyncExternalStore(subscribeComposer, listComposerDropHandlers);
  const [attachments, setAttachments] = useState<ComposerAttachment[]>([]);
  const [composerError, setComposerError] = useState<string | null>(null);
  // Spec 0113 §6a: below 640 px the composer actions sit behind a "+" menu.
  const narrow = useIsNarrow();
  const [composerMenuOpen, setComposerMenuOpen] = useState(false);
  const invokeComposerAction = (action: ComposerAction) => {
    setComposerError(null);
    void Promise.resolve(action.onInvoke(composerCtx)).catch((err) =>
      setComposerError(commandErrorMessage(err)),
    );
  };
  const chatAreaRef = useRef<HTMLDivElement>(null);
  const composerCtx = useMemo<ComposerContext>(
    () => ({
      sessionId,
      addAttachment: (a) =>
        setAttachments((prev) => (prev.some((p) => p.id === a.id) ? prev : [...prev, a])),
      removeAttachment: (id) => setAttachments((prev) => prev.filter((p) => p.id !== id)),
    }),
    [sessionId],
  );
  const composerCtxRef = useRef(composerCtx);
  composerCtxRef.current = composerCtx;
  const dropHandlersRef = useRef(dropHandlers);
  dropHandlersRef.current = dropHandlers;
  const hasDropHandler = dropHandlers.length > 0;
  const [dropOver, setDropOver] = useState(false);
  // Native Drops auf den Chat gehen an die registrierten Handler; ohne
  // Handler bleibt der Listener aus und das Ablegen tut im Chat nichts.
  // Pfade verlassen das Backend nicht (ADR 0113): der Handler holt sie dort.
  useEffect(() => {
    if (!hasDropHandler) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "leave") {
          setDropOver(false);
          return;
        }
        // Only the visible chat area, and only under the pointer.
        const inside = isPointerInside(chatAreaRef.current, payload.position);
        if (payload.type === "over" || payload.type === "enter") {
          setDropOver(inside);
          return;
        }
        setDropOver(false);
        if (!inside) return;
        for (const handler of dropHandlersRef.current) {
          void Promise.resolve(handler(composerCtxRef.current)).catch((err) =>
            setComposerError(commandErrorMessage(err)),
          );
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
      setDropOver(false);
    };
  }, [hasDropHandler]);
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
  const [autoContinuing, setAutoContinuing] = useState(false);
  // Issue #600: provider overloaded, automatic retry pending.
  const [retrying, setRetrying] = useState<{ attempt: number; max: number; deadline: number } | null>(null);
  const [retryNow, setRetryNow] = useState(() => Date.now());
  // Issue #694: a request is out; `since` is the last sign of life (request
  // sent / last content) and drives the "Waiting for a response" line.
  const [aiWait, setAiWait] = useState<{ provider: string; since: number; gotContent: boolean } | null>(null);
  const [waitNow, setWaitNow] = useState(() => Date.now());
  useEffect(() => {
    if (!aiWait) return;
    const id = window.setInterval(() => setWaitNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [aiWait]);
  useEffect(() => {
    if (!retrying) return;
    const id = window.setInterval(() => setRetryNow(Date.now()), 250);
    return () => window.clearInterval(id);
  }, [retrying]);
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
  const [liveOutputs, setLiveOutputs] = useState<Record<string, LiveOutput>>({});
  // Issue #693: injection check state per checked item (action / web card id).
  const [injectionChecks, setInjectionChecks] = useState<Record<string, InjectionCheckState>>({});
  // Issue #327: actions whose command is probably waiting for input.
  const [waitingActions, setWaitingActions] = useState<Record<string, true>>({});
  // Spec 0055, Teil 1: `<textarea>` statt `<input type="text">` für die
  // mehrzeilige Eingabe (Shift+Enter/Auto-Grow) — dieselbe DOM-API
  // (`.value`/`.selectionStart`/`.setSelectionRange`), die restliche
  // Historien-Navigationslogik unten bleibt unverändert.
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const jumpButtonRef = useRef<HTMLButtonElement>(null);
  // Issue #758: the jump button hides once the column is at the bottom. A
  // hidden element drops focus to <body>, so hand focus to the chat input
  // first when the button still has it.
  useLayoutEffect(() => {
    if (atBottom && jumpButtonRef.current && document.activeElement === jumpButtonRef.current) {
      inputRef.current?.focus();
    }
  }, [atBottom]);
  // Spec 0108: Spracheingabe an/aus (Standard an); der erkannte Text geht
  // über `handleDraftChange` in den Entwurf, nie direkt an die KI.
  const [voiceEnabled, setVoiceEnabled] = useState(true);
  const [voicePhase, setVoicePhase] = useState<VoicePhase>("idle");
  const [voiceFocusPending, setVoiceFocusPending] = useState(false);
  const voiceBaseRef = useRef<string | null>(null);
  useEffect(() => {
    loadVoiceInputEnabled()
      .then(setVoiceEnabled)
      .catch(() => {});
    return subscribeVoiceInputEnabled(setVoiceEnabled);
  }, []);
  const draftRef = useRef("");

  // Spec 0015, Abschnitt 5: einmalig pro Server geladen, kein
  // wiederholtes Nachladen bei jeder Pfeiltaste. `historyNav` verfolgt den
  // laufenden Navigations-Modus (s. `promptHistoryNav.ts`) — beim
  // Serverwechsel zurückgesetzt, damit kein Navigations-Zustand auf die
  // History eines anderen Servers zeigt.
  const [promptHistory, setPromptHistory] = useState<string[]>([]);
  const [historyNav, setHistoryNav] = useState<HistoryNavState>(initialHistoryNavState);

  useEffect(() => {
    listPromptHistory(serverId)
      .then(setPromptHistory)
      .catch(() => setPromptHistory([]));
    setHistoryNav(initialHistoryNavState);
  }, [serverId]);

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

  // Spec 0034, Abschnitt 6/8: seedet den bereits geladenen Verlauf einer
  // fortgesetzten Sitzung (leer bei einem frischen `connect()`, s.
  // `get_chat_history`-Doc-Kommentar) — ohne das startet der Tab nach
  // einem "Fortsetzen" fälschlich leer, obwohl das Backend bereits mit
  // gefüllter Historie gestartet ist. Läuft einmal pro Tab (`sessionId`
  // ändert sich nie innerhalb desselben Tabs), vor jedem Live-Event.
  useEffect(() => {
    getChatHistory(sessionId)
      .then((entries) => {
        if (entries.length === 0) return;
        setItems((prev) => [
          ...entries.map(historyEntryToItem),
          ...prev,
        ]);
      })
      .catch(() => {
        // Best-effort: eine fehlgeschlagene Historie-Ladung soll den Tab
        // nicht unbenutzbar machen, nur ohne sichtbaren Altverlauf starten
        // (derselbe Geist wie andere Best-effort-`.catch`-Stellen in
        // dieser Komponente, z. B. `listPromptHistory` oben).
      });
  }, [sessionId]);

  // Issue #1020, Spec 0114 §4: after the app returns from the background the
  // webview may have missed events — rebuild this chat from the backend:
  // history, action results, and the confirmation that still waits.
  // `riskSecondOpinionEnabled` is read through a ref so the listener (and a
  // re-sync in flight) is not torn down when the setting loads.
  const riskSecondOpinionRef = useRef(riskSecondOpinionEnabled);
  useEffect(() => {
    riskSecondOpinionRef.current = riskSecondOpinionEnabled;
  }, [riskSecondOpinionEnabled]);
  useEffect(() => {
    let cancelled = false;
    const onVisible = () => {
      if (document.visibilityState !== "visible") return;
      getChatResync(sessionId)
        .then((resync) => {
          if (cancelled) return;
          const rebuilt = resync.history.map(historyEntryToItem);
          const pending = resync.pendingProposal;
          if (pending) {
            rebuilt.push(actionItemFromProposed(pending, riskSecondOpinionRef.current));
          }
          setItems((prev) => mergeResyncedItems(prev, rebuilt, pending?.actionId ?? null));
        })
        .catch(() => {
          // Best-effort like the initial history load: keep what is shown.
        });
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      cancelled = true;
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [sessionId]);

  useEffect(() => {
    // Spec 0061 Follow-up (spec-reviewer Fund): Timer für das automatische
    // Entfernen der `aiBudgetWaiting`-Karte (s. `onAiBudgetWaiting` unten)
    // dürfen beim Session-Wechsel/Unmount nicht weiterlaufen.
    const pendingBudgetTimers: number[] = [];
    const unlisten = [
      onChatAiRequestStarted((event) => {
        if (event.sessionId !== sessionId) return;
        const now = Date.now();
        setWaitNow(now);
        setAiWait({ provider: event.providerName, since: now, gotContent: false });
      }),
      onChatTextDelta((event) => {
        if (event.sessionId !== sessionId) return;
        setRetrying(null);
        setAiWait((w) => (w ? { ...w, since: Date.now(), gotContent: true } : w));
        setItems((prev) => {
          const last = prev[prev.length - 1];
          if (last?.type === "assistant") {
            return [...prev.slice(0, -1), { ...last, text: last.text + event.delta }];
          }
          return [...prev, { type: "assistant", id: freshId(), text: event.delta }];
        });
      }),
      onChatActionProposed((event) => {
        if (event.sessionId !== sessionId) return;
        setRetrying(null);
        setAiWait(null);
        setItems((prev) => [...prev, actionItemFromProposed(event, riskSecondOpinionRef.current)]);
      }),
      onRiskAssessmentUpdated((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) =>
          prev.map((item) =>
            item.type === "action" && item.actionId === event.actionId && item.riskAssessment
              ? {
                  ...item,
                  riskAssessment: {
                    ...item.riskAssessment,
                    dataRisk: event.dataRisk,
                    dataRiskReason: event.reason,
                    aiReviewed: true,
                  },
                  riskSecondOpinionPending: false,
                  secondOpinionUnavailable: event.secondOpinionUnavailable === true,
                }
              : item,
          ),
        );
      }),
      // Spec 0092, A3.2: die Entscheidung zu `actionId` wurde NACH
      // `chat-action-proposed` von `AutoExec` auf `Confirm` verschärft (die
      // KI-Zweitmeinung hat das Daten-Risiko auf Rot gehoben) — die Karte
      // baut sich daraus selbst ein `Confirm{reason, code}`, weil dieses
      // Event (anders als `chat-action-proposed`) nicht den ganzen
      // `Decision`-Enum überträgt. `needsConfirmation` (s. `ChatItemView`)
      // greift danach automatisch, der Bestätigungsdialog erscheint also
      // ohne weiteres Zutun.
      //
      // `startedAt` MUSS auf `null` zurückgesetzt werden: Beim ursprünglichen
      // `chat-action-proposed` stand die Entscheidung noch auf `AutoExec`,
      // also wurde `startedAt` optimistisch auf `Date.now()` gesetzt (s.
      // unten) — ohne diesen Reset zeigte die Karte fälschlich gleichzeitig
      // den Lauf-Indikator (`RunningCommandIndicator`, „läuft bereits") UND
      // den Bestätigungsdialog, obwohl die Aktion nie automatisch
      // angelaufen ist (A3.1: sie wird wie jedes andere `Confirm`
      // behandelt).
      onActionDecisionEscalated((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) =>
          prev.map((item) =>
            item.type === "action" && item.actionId === event.actionId
              ? {
                  ...item,
                  decision: { Confirm: { reason: event.reason, code: event.code } },
                  startedAt: null,
                }
              : item,
          ),
        );
      }),
      onChatActionOutput((event) => {
        if (event.sessionId !== sessionId) return;
        setLiveOutputs((prev) => ({
          ...prev,
          [event.actionId]: appendLiveOutput(prev[event.actionId], event),
        }));
      }),
      onChatActionWaitingInput((event) => {
        if (event.sessionId !== sessionId) return;
        setWaitingActions((prev) => {
          if (event.waiting) return { ...prev, [event.actionId]: true };
          if (!(event.actionId in prev)) return prev;
          const { [event.actionId]: _cleared, ...rest } = prev;
          return rest;
        });
      }),
      onActionConfirmationSettled((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) =>
          prev.map((item) =>
            item.type === "action" && item.actionId === event.actionId
              ? { ...item, responded: true, settled: event.reason }
              : item,
          ),
        );
        onActionSettledRef.current(sessionId);
      }),
      onChatActionResult((event) => {
        if (event.sessionId !== sessionId) return;
        setWaitingActions((prev) => {
          if (!(event.actionId in prev)) return prev;
          const { [event.actionId]: _finished, ...rest } = prev;
          return rest;
        });
        // Issue #325: the final result replaces the live view.
        setLiveOutputs((prev) => {
          if (!(event.actionId in prev)) return prev;
          const { [event.actionId]: _finished, ...rest } = prev;
          return rest;
        });
        setItems((prev) =>
          prev.map((item) =>
            item.type === "action" && item.actionId === event.actionId
              ? { ...item, result: event.result, responded: true }
              : item,
          ),
        );
      }),
      onChatError((event) => {
        if (event.sessionId !== sessionId) return;
        setRetrying(null);
        setAiWait(null);
        setItems((prev) => {
          // Issue #600: "Try again" only when nothing but the user's own
          // message came since the last user turn (no reply, command or
          // proposal that a resend would repeat or contradict).
          let retryable = isRetryableAiError(event.code);
          let sawUser = false;
          for (let i = prev.length - 1; retryable && i >= 0; i--) {
            const it = prev[i];
            if (it.type === "user") {
              sawUser = true;
              break;
            }
            if (it.type === "assistant" || it.type === "action" || it.type === "error") retryable = false;
          }
          if (!sawUser) retryable = false;
          return [
            ...prev,
            {
              type: "error",
              id: freshId(),
              message: event.message,
              code: event.code ?? null,
              details: {
                providerErrorType: event.providerErrorType,
                httpStatus: event.httpStatus,
                providerMessage: event.providerMessage,
                providerName: event.providerName,
                timeoutSecs: event.timeoutSecs,
                accessReturns: event.accessReturns,
              },
              retryable,
            },
          ];
        });
      }),
      onChatAiRetrying((event) => {
        if (event.sessionId !== sessionId) return;
        setRetryNow(Date.now());
        setRetrying({
          attempt: event.attempt,
          max: event.maxAttempts,
          deadline: Date.now() + event.delaySecs * 1000,
        });
      }),
      onChatResponseTruncated((event) => {
        if (event.sessionId !== sessionId) return;
        // Spec 0065, Teil 2: markiert die zuletzt gestreamte Assistant-
        // Nachricht (dieselbe `last?.type === "assistant"`-Suche wie bei
        // `onChatTextDelta` oben) — das Event kommt immer unmittelbar nach
        // dem letzten Text-Delta derselben Antwort.
        setItems((prev) => {
          const last = prev[prev.length - 1];
          if (last?.type === "assistant") {
            return [...prev.slice(0, -1), { ...last, truncated: true }];
          }
          // Spec 0080, A3, zweiter Punkt: früher wurde das Event hier
          // verworfen, wenn keine Assistant-Nachricht direkt vorherging
          // (z. B. eine Runde, die schon am Limit endet, bevor auch nur ein
          // Text-Delta kam) — der Nutzer sah dann trotz Kürzung gar
          // nichts. Jetzt kommt stattdessen ein leeres Assistant-Element
          // mit Kürzungs-Hinweis + „Weiter" dazu.
          return [...prev, { type: "assistant", id: freshId(), text: "", truncated: true }];
        });
      }),
      onChatInjectionCheck((event) => {
        if (event.sessionId !== sessionId) return;
        const { sessionId: _s, itemId, ...state } = event;
        setInjectionChecks((prev) => ({ ...prev, [itemId]: state as InjectionCheckState }));
      }),
      onChatWebActivity((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [
          ...prev,
          {
            type: "webActivity",
            id: freshId(),
            activity: event.activity,
            activityId: event.activityId,
          },
        ]);
      }),
      onChatResponseEmpty((event) => {
        if (event.sessionId !== sessionId) return;
        // Spec 0080, A3: eigenes Element statt eines markierten Assistant-
        // Eintrags — kein „Weiter" (die Runde ist nicht abgeschnitten,
        // sondern leer), keine Export-/Notiz-Leiste (nichts zu exportieren).
        // Text der NÄCHSTEN Runde landet nie hier: `onChatTextDelta` prüft
        // `last?.type === "assistant"`, "emptyResponse" erfüllt das nie,
        // ein Folge-Delta hängt sich also immer an ein neues Element.
        setItems((prev) => [...prev, { type: "emptyResponse", id: freshId() }]);
      }),
      onChatResponseRefused((event) => {
        if (event.sessionId !== sessionId) return;
        // Issue #647: own item; text streamed before stays as it is.
        setItems((prev) => [...prev, { type: "refusedResponse", id: freshId() }]);
      }),
      onChatWebResearchUnavailable((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [...prev, { type: "webResearchUnavailable", id: freshId() }]);
      }),
      onChatQueuedMessagesSent((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) =>
          prev.map((it) => (it.type === "user" && it.queued ? { ...it, queued: false } : it)),
        );
      }),
      onChatContextUsage((event) => {
        if (event.sessionId !== sessionId) return;
        setContextUsage({ inputTokens: event.inputTokens, windowTokens: event.windowTokens });
      }),
      onChatContextCompacted((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [...prev, { type: "contextCompacted", id: freshId() }]);
      }),
      onChatResponseCancelled((event) => {
        if (event.sessionId !== sessionId) return;
        setRetrying(null);
        setItems((prev) => [...prev, { type: "responseCancelled", id: freshId() }]);
      }),
      onChatAutoContinuationLimitReached((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [
          ...prev,
          { type: "autoContinuationLimitReached", id: freshId(), limit: event.limit },
        ]);
      }),
      onChatAutoContinuationStarted((event) => {
        if (event.sessionId !== sessionId) return;
        setAutoContinuing(true);
      }),
      onAiBudgetWaiting((event) => {
        if (event.sessionId !== sessionId) return;
        const id = freshId();
        setItems((prev) => [...prev, { type: "aiBudgetWaiting", id, waitSeconds: event.waitSeconds }]);
        // Spec 0061 §4: "verschwindet, sobald der Request rausgeht". Es gibt
        // (bewusst, s. ADR 0054) kein eigenes "Warten beendet"-Event vom
        // Backend — die Karte ist rein informativ, kein Steuerelement. Ein
        // client-seitiger Timer approximiert das Verschwinden anhand der
        // bekannten Wartedauer, statt die Karte dauerhaft stehen zu lassen
        // (spec-reviewer Fund: Karten stapelten sich sonst über mehrere
        // Wartevorgänge, z. B. innerhalb einer Auto-Fortsetzungs-Serie).
        pendingBudgetTimers.push(
          window.setTimeout(
            () => setItems((prev) => prev.filter((it) => it.id !== id)),
            Math.max(event.waitSeconds, 1) * 1000,
          ),
        );
      }),
      onChatQuestionAsked((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [
          ...prev,
          {
            type: "question",
            id: freshId(),
            questionId: event.questionId,
            question: event.question,
            options: event.options,
            multiSelect: event.multiSelect,
          },
        ]);
      }),
      onChatDocumentGenerated((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [
          ...prev,
          {
            type: "document",
            id: freshId(),
            title: event.title,
            contentMarkdown: event.contentMarkdown,
          },
        ]);
      }),
    ];

    return () => {
      unlisten.forEach((p) => p.then((unlistenFn) => unlistenFn()));
      pendingBudgetTimers.forEach((id) => window.clearTimeout(id));
    };
  }, [sessionId]);

  const respond = (actionId: string, decision: ActionUserDecision) => {
    // Spec 0027: die tatsächliche Ausführung beginnt jetzt (nicht bei
    // "deny", das führt nie etwas aus) — Startzeitpunkt für den
    // Lauf-Indikator.
    const startedAt =
      decision.decision === "approve" || decision.decision === "editThenApprove"
        ? Date.now()
        : null;
    setItems((prev) =>
      prev.map((item) =>
        item.type === "action" && item.actionId === actionId
          ? { ...item, responded: true, userDecision: decision.decision, startedAt: startedAt ?? item.startedAt }
          : item,
      ),
    );
    onActionSettled(sessionId);
    respondToAction(sessionId, actionId, decision).catch((err) => {
      // Issue #1019: the optimistic state is rolled back. An unknown
      // confirmation was settled by the backend: the card shows "expired",
      // never "approved". Any other error leaves the request open.
      const unknown = commandErrorCode(err) === "ACTION_CONFIRMATION_UNKNOWN";
      setItems((prev) => [
        ...prev.map((item) =>
          item.type === "action" && item.actionId === actionId
            ? unknown
              ? { ...item, responded: true, userDecision: undefined, startedAt: null, settled: "unknown" as const }
              : { ...item, responded: false, userDecision: undefined, startedAt: null }
            : item,
        ),
        { type: "error" as const, id: freshId(), message: commandErrorMessage(err), code: null },
      ]);
    });
  };

  /** Spec 0021, Abschnitt 5 / Spec 0066, §1: bricht den laufenden
   * KI-Request sofort ab und verhindert weitere automatische Runden. Ein
   * offener Bestätigungsdialog bleibt stehen, ein laufendes Kommando läuft
   * zu Ende. */
  const handleStopAutoContinuation = () => {
    setAutoContinuing(false);
    setAiWait(null);
    stopAutoContinuation(sessionId).catch((err) =>
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err), code: null },
      ]),
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
    setItems((prev) =>
      prev.map((item) =>
        item.type === "action" && item.actionId === actionId
          ? { ...item, responded: true, startedAt: Date.now() }
          : item,
      ),
    );
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
          setItems((prev) => [
            ...prev,
            {
              type: "ruleNotice",
              id: freshId(),
              reasons: verdict.reasons.map((r) => r.code),
            },
          ]);
        }
      })
      .catch((err) =>
      setItems((prev) => [
        ...prev,
        // Spec 0077, 3.1.4: Code weiterreichen statt `null`, damit ein
        // ungültiges Muster auch hier übersetzt erscheint. Die Anzeige
        // ersetzt die `message` durch den übersetzten Satz — hier bewusst
        // ohne den Fehlertext der Bibliothek: Es wird ein Vorschlag
        // angelegt, kein selbstgeschriebenes Muster.
        { type: "error", id: freshId(), message: commandErrorMessage(err), code: commandErrorCode(err) },
      ]),
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
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err), code: null },
      ]);
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
    setItems((prev) =>
      prev.map((it) => (it.id === itemId && it.type === "assistant" ? { ...it, truncated: false } : it)),
    );
    await runSend(() => continueTruncatedResponse(sessionId));
  };

  /** Issue #600: "Try again" on a retryable error card. */
  const handleRetryTurn = async (itemId: string) => {
    if (sending) return;
    setItems((prev) => prev.filter((it) => it.id !== itemId));
    await runSend(() => retryChatTurn(sessionId));
  };

  const runSend = async (call: () => Promise<void>) => {
    pendingSendsRef.current += 1;
    // Issue #694: drop a stale wait line from the previous turn.
    if (pendingSendsRef.current === 1) setAiWait(null);
    setSending(true);
    try {
      await call();
    } catch (err) {
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err), code: null },
      ]);
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
        setAutoContinuing(false);
        setRetrying(null);
        setAiWait(null);
        // Läuft im Backend nichts mehr, wartet auch keine Nachricht mehr.
        setItems((prev) =>
          prev.map((it) => (it.type === "user" && it.queued ? { ...it, queued: false } : it)),
        );
      }
    }
  };

  /** Issue #547: an option click is a normal user message through the
   * existing send path — never an action response. */
  const handleQuestionAnswer = async (text: string) => {
    const queued = sending;
    scrollToBottom();
    setItems((prev) => [
      ...prev,
      { type: "user", id: freshId(), text, ...(queued ? { queued: true } : {}) },
    ]);
    await runSend(() => sendChatMessage(sessionId, text));
  };

  const questionStates = deriveQuestionStates(items);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const text = draft.trim();
    if (!text || voicePhase !== "idle") return;
    const sentAttachments = attachments;
    setComposerError(null);
    // Spec 0066, §2: läuft schon eine Antwort, reiht das Backend die
    // Nachricht ein (kehrt sofort zurück) und schickt sie mit der nächsten
    // Anfrage an die KI — die laufende Antwort läuft weiter.
    const queued = sending;
    const itemId = freshId();
    // Issue #540: an own message always jumps to the bottom and follows again.
    scrollToBottom();
    setItems((prev) => [
      ...prev,
      {
        type: "user",
        id: itemId,
        text,
        ...(queued ? { queued: true } : {}),
        ...(sentAttachments.length > 0
          ? { attachments: sentAttachments.map((a) => ({ name: a.name, kind: a.kind })) }
          : {}),
      },
    ]);
    setDraft("");
    setAttachments([]);
    setHistoryNav(initialHistoryNavState);
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
        void listPromptHistory(serverId)
          .then(setPromptHistory)
          .catch(() => {});
      } catch (err) {
        // Issue #315: „Neuer Chat" tauscht den Chat gerade aus; das Backend hat
        // die Nachricht abgelehnt. Sie verschwindet aus dem Verlauf und bleibt
        // im Eingabefeld (ein inzwischen getippter Entwurf bleibt unberührt).
        if (commandErrorCode(err) === "CHAT_SWITCHING") {
          setItems((prev) => prev.filter((it) => it.id !== itemId));
          setDraft((current) => (current === "" ? text : current));
        }
        // Issue #471: lehnt das Backend eine Nachricht mit Anhängen ab
        // (Grenze, Modell, gesperrte Funktion), wurde nichts gesendet: Text
        // und Chips bleiben zum Korrigieren, der Fehler steht am Eingabefeld.
        if (sentAttachments.length > 0) {
          setItems((prev) => prev.filter((it) => it.id !== itemId));
          setDraft((current) => (current === "" ? text : current));
          setAttachments((current) => (current.length === 0 ? sentAttachments : current));
          setComposerError(commandErrorMessage(err));
          return;
        }
        throw err;
      }
    });
  };

  // Spec 0015, Abschnitt 5: Pfeil-oben/-unten lösen Historien-Navigation nur
  // an den jeweiligen Feldrändern aus (Cursor-Position-Gate hier, reine
  // Index-Logik in `navigateHistory`) — sonst läuft die normale
  // Cursor-Bewegung des Browsers unverändert durch (kein `preventDefault`).
  const pendingCaretToEndRef = useRef(false);

  const handleInputKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Spec 0055, Teil 1: Standard-Chat-Verhalten — Enter sendet,
    // Shift+Enter fügt einen Zeilenumbruch ein.
    //
    // Spec-Reviewer-Fund (Spec 0055, Review des Gesamtpakets):
    // `isComposing`/`keyCode === 229` — während einer IME-Komposition
    // (z. B. Pinyin/Kanji-Eingabe, aber auch macOS-Tot-Tasten für Akzente)
    // bestätigt Enter erst die Kandidatenauswahl, sendet noch nichts.
    // Ohne diese Prüfung würde der erste Enter die halbfertige Eingabe
    // sofort abschicken statt nur die Komposition abzuschließen —
    // "plattformübliches Verhalten" (Spec, Teil 1) verlangt genau diese
    // Rücksicht.
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) {
      e.preventDefault();
      // `requestSubmit()` statt eines direkten `handleSubmit(e)`-Aufrufs:
      // `handleSubmit` ist auf `FormEvent` typisiert (liest `draft`/
      // `sending` ohnehin aus dem Komponenten-State, nicht aus dem
      // Event) — über das Formular selbst einzureichen nutzt denselben
      // Pfad wie ein Klick auf "Senden", statt die Sende-Logik ein
      // zweites Mal zu duplizieren.
      //
      // Spec-Reviewer-Fund: `HTMLFormElement.requestSubmit` fehlt in
      // älteren WebKit-Versionen (vor Safari 16/macOS 13) — ein rein
      // optionaler Verkettungs-Operator (`form?.requestSubmit()`) schützt
      // nur gegen ein fehlendes `<form>`, nicht gegen eine fehlende
      // Methode darauf, und hätte Enter auf so einer Plattform sang- und
      // klanglos wirkungslos gemacht (der bisherige `<input>`-Pfad hatte
      // dieses Problem nicht, weil dort das native Browser-Default-Submit
      // griff). `dispatchEvent` mit einem synthetischen `submit`-Ereignis
      // ist der Standard-Fallback dafür — dasselbe Ereignis, auf das
      // React's `onSubmit` ohnehin hört.
      const form = e.currentTarget.form;
      if (form) {
        if (typeof form.requestSubmit === "function") {
          form.requestSubmit();
        } else {
          form.dispatchEvent(new Event("submit", { cancelable: true, bubbles: true }));
        }
      }
      return;
    }
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    const input = e.currentTarget;
    const atStart = input.selectionStart === 0 && input.selectionEnd === 0;
    const atEnd =
      input.selectionStart === input.value.length && input.selectionEnd === input.value.length;
    if (e.key === "ArrowUp" && !atStart) return;
    if (e.key === "ArrowDown" && !atEnd) return;

    const result = navigateHistory(
      e.key === "ArrowUp" ? "up" : "down",
      promptHistory,
      historyNav,
      draft,
    );
    if (!result) return;

    e.preventDefault();
    setHistoryNav(result.nextState);
    setDraft(result.value);
    pendingCaretToEndRef.current = true;
  };

  // Setzt den Cursor nach dem Einsetzen eines Historieneintrags ans Ende
  // (Abschnitt 5: von der Spec nicht explizit vorgegeben, üblicher/
  // sinnvoller Default). Läuft absichtlich erst *nach* dem Commit des neuen
  // `draft`-Werts (statt synchron im Keydown-Handler) — nur dann steht der
  // tatsächliche DOM-Wert schon fest, gegen den `setSelectionRange`
  // rechnen muss. Der Ref-Flag verhindert, dass normales Tippen (das
  // `draft` ebenfalls ändert) den Cursor fälschlich ans Ende zwingt.
  useEffect(() => {
    if (pendingCaretToEndRef.current && inputRef.current) {
      const pos = inputRef.current.value.length;
      inputRef.current.setSelectionRange(pos, pos);
      pendingCaretToEndRef.current = false;
    }
  }, [draft]);

  // Spec 0055, Teil 1: "Feld wächst mit dem Inhalt (auto-grow bis
  // sinnvolle Maxhöhe, dann intern scrollbar)". Höhe bei jeder
  // `draft`-Änderung neu berechnen statt per CSS `field-sizing`
  // (Browser-Unterstützung dafür ist noch zu neu für die von Tauris
  // WebView abgedeckte Plattformbreite) — `style.height = "auto"` vor dem
  // Auslesen von `scrollHeight` ist nötig, sonst würde eine gekürzte
  // Eingabe (z. B. nach Rückgängig-Historie-Navigation) die alte,
  // größere Höhe nie wieder verkleinern. Tailwinds `max-h-40` (s.
  // `className` unten, 10rem/160px — rund 8 Zeilen bei `text-sm`) deckelt
  // das Wachstum; `overflow-y-auto` auf demselben Element übernimmt
  // darüber das interne Scrollen.
  useEffect(() => {
    const textarea = inputRef.current;
    if (!textarea) return;
    textarea.style.height = "auto";
    textarea.style.height = `${textarea.scrollHeight}px`;
  }, [draft]);

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
      setContextUsage((prev) => (prev ? { ...prev, inputTokens: 0 } : prev));
      setItems([]);
      setHistoryNav(initialHistoryNavState);
    } catch (err) {
      setNewChatError(commandErrorMessage(err));
    } finally {
      setStartingNewChat(false);
    }
  };

  const handleDraftChange = (value: string) => {
    setDraft(value);
    // Spec 0015, Abschnitt 5, bewusste MVP-Vereinfachung: jede normale
    // Texteingabe beendet den Navigations-Modus (kein volles
    // Readline-Verhalten). Feuert nie für die programmatischen
    // `setDraft`-Aufrufe aus `handleInputKeyDown` selbst — React-`onChange`
    // löst nur bei echten Browser-Eingabe-Events aus, nicht bei
    // State-Updates.
    if (historyNav.historyIndex !== null) {
      setHistoryNav(initialHistoryNavState);
    }
  };

  draftRef.current = draft;
  // Spec 0108 §5.2: live text. The draft before Start is kept; provisional
  // text replaces everything after it, so no merging with edits is needed
  // (the input is read-only meanwhile).
  const voiceHandlers = useMemo<VoiceHandlers>(() => {
    const showAfterBase = (text: string) => {
      const base = voiceBaseRef.current;
      if (base === null) return;
      setDraft(appendToDraft(base, text));
    };
    return {
      onStart: () => {
        voiceBaseRef.current = draftRef.current;
      },
      onPartial: showAfterBase,
      onFinal: (text) => {
        showAfterBase(text);
        voiceBaseRef.current = null;
        setVoiceFocusPending(true);
      },
      onFailed: () => {
        voiceBaseRef.current = null;
        setVoiceFocusPending(true);
      },
      onCancel: () => {
        if (voiceBaseRef.current !== null) setDraft(voiceBaseRef.current);
        voiceBaseRef.current = null;
      },
    };
  }, []);
  const voiceLocked = voicePhase !== "idle";
  // Once the input is editable again: focus it, caret at the end.
  useEffect(() => {
    if (!voiceFocusPending || voiceLocked) return;
    setVoiceFocusPending(false);
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
  }, [voiceFocusPending, voiceLocked]);

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
        <div className="border-t border-border bg-surface-raised">
          {composerError !== null && (
            <p role="alert" className="px-3 pt-2 text-xs text-danger-fg">
              {composerError}
            </p>
          )}
          {attachments.length > 0 && (
            <ul aria-label={t("chat.attachmentsLabel")} className="flex flex-wrap gap-2 px-3 pt-2">
              {attachments.map((a) => (
                <li
                  key={a.id}
                  className="flex items-center gap-2 rounded border border-border-strong bg-border px-2 py-1 text-xs text-fg"
                >
                  <span>
                    {a.name} · {formatBytes(a.size)}
                  </span>
                  {a.unredactedHint && (
                    <span className="text-warning-fg-soft">{t("chat.attachmentUnredacted")}</span>
                  )}
                  <button
                    type="button"
                    aria-label={t("chat.attachmentRemove", { name: a.name })}
                    onClick={() => composerCtx.removeAttachment(a.id)}
                    className="text-fg-muted hover:text-fg-bright focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
                  >
                    ×
                  </button>
                </li>
              ))}
            </ul>
          )}
        {contextUsage && (
          <ContextUsageIndicator
            inputTokens={contextUsage.inputTokens}
            windowTokens={contextUsage.windowTokens}
          />
        )}
        <form onSubmit={handleSubmit} className="flex flex-wrap items-end gap-2 p-3 narrow:[&>textarea]:order-first narrow:[&>textarea]:basis-full">
          <span className="self-start narrow:hidden py-1.5 font-mono text-sm leading-5 text-accent-strong">&gt;</span>
          {/* Spec 0055, Teil 1: Enter sendet, Shift+Enter fügt einen
              Zeilenumbruch ein (s. `handleInputKeyDown`); `rows={1}` +
              `resize-none` + die Höhen-Effekt oben ergeben das
              Auto-Grow-bis-`max-h-40`-dann-scrollbar-Verhalten, `align`
              bleibt sonst wie beim bisherigen `<input>`. */}
          <textarea
            ref={inputRef}
            rows={1}
            value={draft}
            readOnly={voiceLocked}
            data-provisional={voiceLocked ? "true" : undefined}
            onChange={(e) => handleDraftChange(e.target.value)}
            onKeyDown={handleInputKeyDown}
            placeholder={t("chat.inputPlaceholder")}
            className={`max-h-40 min-w-0 flex-1 resize-none overflow-y-auto bg-transparent py-1.5 text-sm leading-5 ${voiceLocked ? "italic text-fg-subtle" : "text-fg"} placeholder:text-fg-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg`}
          />
          {narrow ? (
            composerActions.length > 0 && (
              <div className="relative">
                <Button
                  aria-label={t("chat.composerMore")}
                  aria-expanded={composerMenuOpen}
                  onClick={() => setComposerMenuOpen((o) => !o)}
                  className="min-h-11 min-w-11"
                >
                  +
                </Button>
                {composerMenuOpen && (
                  <div className="absolute bottom-full left-0 z-20 mb-1 flex w-56 max-w-[calc(100vw-1.5rem)] flex-col gap-1 rounded border border-border bg-surface p-2 shadow-lg">
                    {composerActions.map((action) => (
                      <ErrorBoundary key={action.id} id={`composer-action:${action.id}`}><Button
                        title={action.disabled ? (action.disabledReason ?? action.label) : action.label}
                        disabled={action.disabled}
                        onClick={() => {
                          setComposerMenuOpen(false);
                          invokeComposerAction(action);
                        }}
                        className="min-h-11 w-full justify-start gap-2 text-left"
                      >
                        <span aria-hidden="true">{action.icon}</span>
                        {action.label}
                      </Button></ErrorBoundary>
                    ))}
                  </div>
                )}
              </div>
            )
          ) : (
            composerActions.map((action) => (
              <ErrorBoundary key={action.id} id={`composer-action:${action.id}`}><button
                type="button"
                aria-label={action.label}
                title={action.disabled ? (action.disabledReason ?? action.label) : action.label}
                disabled={action.disabled}
                onClick={() => invokeComposerAction(action)}
                className="rounded p-1.5 text-fg-muted hover:bg-border hover:text-fg-bright focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg disabled:opacity-50"
              >
                {action.icon}
              </button></ErrorBoundary>
            ))
          )}
          {voiceEnabled && (
            <VoiceInputControl
              handlers={voiceHandlers}
              onPhaseChange={setVoicePhase}
              panelRef={chatAreaRef}
            />
          )}
          <Button
            type="submit"
            variant="primary"
            className="narrow:ml-auto narrow:min-h-11 narrow:min-w-20"
            disabled={draft.trim().length === 0 || voiceLocked}
          >
            {t("chat.send")}
          </Button>
        </form>
        </div>
      )}
    </div>
  );
}
