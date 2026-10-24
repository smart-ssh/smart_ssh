// Spec 0017: zentrale Buchführung über offene Session-Tabs. Seit Spec 0104
// gilt "ein Tab pro Server" nur für Nutzer-Tabs: MCP-Clients bekommen je
// Server einen eigenen MCP-Tab (`SessionTab.mcp`). Bewusst
// getrennt vom eigentlichen Chat-/Terminal-Zustand (der bleibt lokal in
// `ChatPanel`/`TerminalView`, s. dortige Doc-Kommentare) — dieser Hook hält
// nur, was die Tab-Leiste selbst zum Rendern braucht: welche Sessions
// offen sind, ihr Servername/Status, und ob eine Bestätigung aussteht
// (Abschnitt 5, Tab-Indikator). Jede Zuordnung hier erfolgt strikt über
// `event.sessionId` aus den jeweiligen Events, nie implizit über den
// gerade aktiven Tab (Abschnitt 4, "das ist der wahrscheinlichste
// Fehlerfall bei dieser Umstellung").
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  cancelConnect,
  commandErrorCode,
  commandErrorConnectLog,
  commandErrorMessage,
  connect,
  disconnect,
  getServer,
  listSessions,
  respondToAction,
  resumeChatSession,
} from "./api";
import { translateErrorCode } from "./errorCodes";
import {
  onActionDecisionEscalated,
  onChatActionProposed,
  onChatSessionChanged,
  onChatActionResult,
  onConnectProgress,
  onConnectionStatusChanged,
  onMcpActionTabRequested,
} from "./events";
import { forgetChatSessionId, setChatSessionId } from "./chatSessionIds";
import { clearHostKeyPrompt } from "./hostKeyPromptBus";
import type {
  ConnectStepRecord,
  ConnectStepSnapshot,
  ConnectionStatus,
  McpSessionInfo,
} from "./types";

/** Issue #548: lifecycle of a tab. `connecting`/`failed` tabs are created
 * right when a connect starts and turn into the `connected` session tab in
 * place; only then is `sessionId` a real session id. */
export type TabPhase = "connecting" | "failed" | "connected";

export interface ConnectFailure {
  message: string;
  /** Detailed step log of the failed attempt (Issue #51), if any. */
  steps: ConnectStepRecord[];
}

/** Backend code of an attempt the user cancelled — never shown as an error. */
export const CONNECT_CANCELLED_CODE = "SSH_CONNECT_CANCELLED";

export interface SessionTab {
  sessionId: string;
  serverId: string;
  serverName: string;
  status: ConnectionStatus;
  /** Grundlage für den Hinweis-Indikator (Abschnitt 5) — auch nach einem
   * Frontend-Reload korrekt (kommt dann aus `list_sessions()`s
   * `hasPendingAction`), unabhängig davon, ob `pendingActionId` bekannt
   * ist. */
  hasPendingAction: boolean;
  /** Die `actionId` der wartenden Bestätigung, sofern dieser
   * Frontend-Prozess das zugehörige `chat-action-proposed`-Event selbst
   * empfangen hat. `null` trotz `hasPendingAction: true` kann nur nach
   * einem Reload vorkommen (Tab aus `list_sessions()` wiederhergestellt,
   * das Event selbst kam vor dem Reload und ist verloren) — dann kann "Tab
   * schließen = ablehnen" (Abschnitt 5, letzter Punkt) die Aktion nicht
   * mehr gezielt per `respondToAction` auflösen, informiert den Nutzer aber
   * weiterhin per Rückfrage. */
  pendingActionId: string | null;
  /** Spec 0104: `null` für einen Nutzer-Tab, sonst die MCP-Sitzung eines
   * externen Clients — eigener Tab je Server und Client, ohne Chat-Eingabe
   * und Terminal, nie Ziel von `findExistingSessionId`. */
  mcp: McpSessionInfo | null;
  /** Issue #657: ordinal among the open user tabs of the same server
   * (1 = the first one); `undefined` for MCP tabs. */
  serverNumber?: number;
  /** Issue #548. */
  phase: TabPhase;
  /** Issue #548: id of the running connect attempt (`connecting` only). */
  attemptId: string | null;
  /** Issue #568: chat session to resume; every attempt (incl. Retry) of
   * this tab resumes the same one. Absent for a plain connect. */
  resumeChatSessionId?: string | null;
  /** Issue #548: live step list from `connect-progress`. */
  progress: ConnectStepSnapshot[];
  /** Issue #548: set in phase `failed`. */
  failure: ConnectFailure | null;
}

/** Issue #657: "name", "name (2)", "name (3)" for several tabs of one server. */
export function numberedServerName(tab: Pick<SessionTab, "serverName" | "serverNumber">): string {
  return tab.serverNumber && tab.serverNumber > 1
    ? `${tab.serverName} (${tab.serverNumber})`
    : tab.serverName;
}

/** Issue #657: smallest number >= 1 not used by an open user tab of the server. */
export function nextServerNumber(
  tabs: readonly Pick<SessionTab, "serverId" | "mcp" | "serverNumber">[],
  serverId: string,
): number {
  const used = new Set(
    tabs.filter((t) => t.serverId === serverId && !t.mcp).map((t) => t.serverNumber),
  );
  let n = 1;
  while (used.has(n)) n++;
  return n;
}

/** Issue #657: among `candidates`, the tab used most recently; `mru` lists
 * session ids, most recent first. Tabs not in `mru` rank last (first wins). */
export function mostRecentlyUsed<T extends { sessionId: string }>(
  candidates: readonly T[],
  mru: readonly string[],
): T | undefined {
  let best: T | undefined;
  let bestRank = Infinity;
  for (const c of candidates) {
    const i = mru.indexOf(c.sessionId);
    const rank = i < 0 ? Number.MAX_SAFE_INTEGER : i;
    if (best === undefined || rank < bestRank) {
      best = c;
      bestRank = rank;
    }
  }
  return best;
}

/** Spec 0104: Beschriftung "<Client> @ <Server>" für MCP-Tabs. */
export function sessionTabLabel(
  tab: Pick<SessionTab, "serverName" | "mcp" | "serverNumber">,
  unnamedClientLabel: string,
): string {
  if (!tab.mcp) return numberedServerName(tab);
  return `${tab.mcp.clientName ?? unnamedClientLabel} @ ${tab.serverName}`;
}

/** Spec 0104, §3: Nachfolger, wenn der aktive Tab `closedSessionId`
 * geschlossen wird — der letzte verbliebene **Nutzer**-Tab, sonst die
 * Übersicht (`null`). Ein MCP-Tab wird nie von selbst aktiv, auch nicht
 * hier; nur ein expliziter Klick aktiviert ihn. */
export function nextActiveAfterClose(
  tabs: readonly Pick<SessionTab, "sessionId" | "mcp">[],
  closedSessionId: string,
): string | null {
  const userTabs = tabs.filter((t) => t.sessionId !== closedSessionId && !t.mcp);
  return userTabs.length > 0 ? userTabs[userTabs.length - 1].sessionId : null;
}

const CONNECTED_FIELDS: Pick<SessionTab, "phase" | "attemptId" | "progress" | "failure"> = {
  phase: "connected",
  attemptId: null,
  progress: [],
  failure: null,
};

export function useSessionTabs() {
  const { t: translate } = useTranslation();
  const [tabs, setTabs] = useState<SessionTab[]>([]);
  // `null` heißt "Übersicht" (Server-/Verwaltungs-Screens) ist aktiv, nicht
  // "keine Tabs offen" — beides ist gleichzeitig möglich (Tabs bleiben im
  // Hintergrund offen, während der Nutzer einen weiteren Server sucht).
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  // Zuletzt gerenderter Tab-Stand für Callbacks, die erst nach einem `await`
  // weiterlaufen (`requestCloseTab`) und sonst einen veralteten `tabs`-Wert
  // aus ihrer Closure lesen würden.
  const tabsRef = useRef(tabs);
  useEffect(() => {
    tabsRef.current = tabs;
  }, [tabs]);
  // Issue #657: session ids, most recently focused first.
  const mruRef = useRef<string[]>([]);
  useEffect(() => {
    if (activeSessionId === null) return;
    mruRef.current = [activeSessionId, ...mruRef.current.filter((id) => id !== activeSessionId)];
  }, [activeSessionId]);

  // Spec 0017, Abschnitt 2: Backend ist die maßgebliche Quelle offener
  // Sessions — beim Start (bzw. Dev-Modus-Hot-Reload) wird die Tab-Leiste
  // daraus wiederhergestellt statt von einem leeren Zustand auszugehen.
  useEffect(() => {
    listSessions()
      .then((summaries) => {
        if (summaries.length === 0) return;
        for (const s of summaries) setChatSessionId(s.sessionId, s.chatSessionId ?? null);
        const restored: SessionTab[] = [];
        for (const s of summaries) {
          restored.push({
            sessionId: s.sessionId,
            serverId: s.serverId,
            serverName: s.serverName,
            status: s.status,
            hasPendingAction: s.hasPendingAction,
            // Die konkrete `actionId` kennt nur ein noch laufender
            // Frontend-Prozess (aus dem `chat-action-proposed`-Event selbst)
            // — nach einem Reload ist sie verloren, s. `SessionTab`-Doc.
            pendingActionId: null,
            mcp: s.mcp ?? null,
            ...(s.mcp ? {} : { serverNumber: nextServerNumber(restored, s.serverId) }),
            ...CONNECTED_FIELDS,
          });
        }
        setTabs(restored);
        // Spec 0104, §3: ein MCP-Tab wird nie von selbst aktiv — gibt es
        // nur MCP-Tabs, bleibt die Übersicht aktiv.
        const firstUserTab = summaries.find((s) => !s.mcp);
        setActiveSessionId((prev) => prev ?? firstUserTab?.sessionId ?? null);
      })
      .catch((err) => console.error(commandErrorMessage(err)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const unlisten = [
      onConnectionStatusChanged((event) => {
        setTabs((prev) =>
          prev.map((t) => (t.sessionId === event.sessionId ? { ...t, status: event.status } : t)),
        );
      }),
      // Issue #611: not tied to a tab — the event for a new session comes
      // before its tab is connected.
      onChatSessionChanged((event) => setChatSessionId(event.sessionId, event.chatSessionId)),
      onConnectProgress((event) => {
        setTabs((prev) =>
          prev.map((t) =>
            t.phase === "connecting" && t.attemptId === event.attemptId
              ? { ...t, progress: event.steps }
              : t,
          ),
        );
      }),
      onChatActionProposed((event) => {
        if (typeof event.decision !== "object" || !("Confirm" in event.decision)) return;
        setTabs((prev) =>
          prev.map((t) =>
            t.sessionId === event.sessionId
              ? { ...t, hasPendingAction: true, pendingActionId: event.actionId }
              : t,
          ),
        );
      }),
      // Spec 0092, A3.6: dieselbe Reaktion wie beim `Confirm`-Zweig von
      // `onChatActionProposed` oben — nur kommt die Verschärfung hier ERST
      // nach `chat-action-proposed` (ursprünglich `AutoExec`, dann per KI-
      // Zweitmeinung auf Rot gehoben). Ohne diesen Handler bliebe der
      // Tab-Hinweis aus, und "Tab schließen" würde weiterhin nichts
      // ablehnen, obwohl eine Bestätigung wartet.
      onActionDecisionEscalated((event) => {
        setTabs((prev) =>
          prev.map((t) =>
            t.sessionId === event.sessionId
              ? { ...t, hasPendingAction: true, pendingActionId: event.actionId }
              : t,
          ),
        );
      }),
      onChatActionResult((event) => {
        setTabs((prev) =>
          prev.map((t) =>
            t.sessionId === event.sessionId && t.pendingActionId === event.actionId
              ? { ...t, hasPendingAction: false, pendingActionId: null }
              : t,
          ),
        );
      }),
      onMcpActionTabRequested((event) => {
        setTabs((prev) => {
          if (prev.some((t) => t.sessionId === event.sessionId)) return prev;
          return [
            ...prev,
            {
              sessionId: event.sessionId,
              serverId: event.serverId,
              // Servername unbekannt, bis `getServer()` unten antwortet —
              // Platzhalter, damit der Tab sofort sichtbar ist (Spec 0028,
              // Abschnitt 9a: "Zielserver ist immer eindeutig sichtbar").
              serverName: "…",
              // Noch nicht `"connected"`: der eigentliche Verbindungsaufbau
              // (inkl. möglichem Host-Key-Dialog) läuft erst nach diesem
              // Event — `onConnectionStatusChanged` aktualisiert den
              // tatsächlichen Status, sobald er feststeht.
              status: "disconnected",
              hasPendingAction: false,
              pendingActionId: null,
              mcp: { clientName: event.clientName ?? null },
              ...CONNECTED_FIELDS,
            },
          ];
        });
        // Spec 0104, §3: kein Fokus-Wechsel — der Tab erscheint im
        // Hintergrund, der aktive Tab des Nutzers bleibt aktiv. Eine
        // wartende Bestätigung signalisieren der Tab-Indikator und die
        // OS-Benachrichtigung (`mcp_backend::notify_pending_confirmation`).
        getServer(event.serverId)
          .then((server) => {
            setTabs((prev) =>
              prev.map((t) =>
                t.sessionId === event.sessionId ? { ...t, serverName: server.name } : t,
              ),
            );
          })
          .catch((err) => console.error(commandErrorMessage(err)));
      }),
    ];
    return () => {
      unlisten.forEach((p) => p.then((fn) => fn()));
    };
  }, []);

  /** Neuer Tab nach erfolgreichem `connect()` (`ServerList`) — Aufrufer hat
   * bereits geprüft, dass für `serverId` noch kein Tab existiert
   * (`findExistingSessionId`). */
  const openTab = (sessionId: string, serverId: string, serverName: string) => {
    setTabs((prev) => {
      if (prev.some((t) => t.sessionId === sessionId)) return prev;
      return [
        ...prev,
        {
          sessionId,
          serverId,
          serverName,
          status: "connected",
          hasPendingAction: false,
          pendingActionId: null,
          mcp: null,
          serverNumber: nextServerNumber(prev, serverId),
          ...CONNECTED_FIELDS,
        },
      ];
    });
    setActiveSessionId(sessionId);
  };

  /** Issue #548: runs one connect attempt for the tab `key`. The tab turns
   * into the session tab in place on success, shows the failure on error and
   * is left alone if the attempt was cancelled / the tab closed meanwhile. */
  const runAttempt = async (
    key: string,
    serverId: string,
    attemptId: string,
    resumeSessionId?: string | null,
  ) => {
    const stillOwnsTab = () =>
      tabsRef.current.some((t) => t.sessionId === key && t.attemptId === attemptId);
    try {
      const sessionId = resumeSessionId
        ? await resumeChatSession(serverId, resumeSessionId, attemptId)
        : await connect(serverId, attemptId);
      if (!stillOwnsTab()) {
        // Closed while the connect was finishing: do not leak the session.
        disconnect(sessionId).catch((err) => console.error(commandErrorMessage(err)));
        return;
      }
      setTabs((prev) =>
        prev.map((t) =>
          t.sessionId === key && t.attemptId === attemptId
            ? {
                ...t,
                sessionId,
                status: "connected",
                phase: "connected",
                attemptId: null,
                progress: [],
                failure: null,
              }
            : t,
        ),
      );
      setActiveSessionId((prev) => (prev === key ? sessionId : prev));
    } catch (err) {
      if (!stillOwnsTab()) return;
      if (commandErrorCode(err) === CONNECT_CANCELLED_CODE) {
        removeTabAndPickNextActive(key);
        return;
      }
      const failure: ConnectFailure = {
        message: translateErrorCode(
          translate,
          commandErrorCode(err),
          commandErrorMessage(err),
        ),
        steps: commandErrorConnectLog(err) ?? [],
      };
      setTabs((prev) =>
        prev.map((t) =>
          t.sessionId === key && t.attemptId === attemptId
            ? { ...t, phase: "failed", attemptId: null, failure }
            : t,
        ),
      );
    } finally {
      clearHostKeyPrompt();
    }
  };

  /** Issue #548: opens and focuses a "Connecting to …" tab and starts the
   * attempt. */
  const startConnect = (
    server: { id: string; name: string },
    resumeChatSessionId?: string,
  ) => {
    const attemptId = crypto.randomUUID();
    const key = `connecting:${attemptId}`;
    const tab: SessionTab = {
      sessionId: key,
      serverId: server.id,
      serverName: server.name,
      status: "disconnected",
      hasPendingAction: false,
      pendingActionId: null,
      mcp: null,
      serverNumber: nextServerNumber(tabsRef.current, server.id),
      phase: "connecting",
      attemptId,
      ...(resumeChatSessionId ? { resumeChatSessionId } : {}),
      progress: [],
      failure: null,
    };
    setTabs((prev) => [...prev, tab]);
    // `tabsRef` is otherwise refreshed only after a render; `runAttempt`
    // may be resumed before that.
    tabsRef.current = [...tabsRef.current, tab];
    setActiveSessionId(key);
    void runAttempt(key, server.id, attemptId, resumeChatSessionId);
  };

  /** Issue #548: "Retry" in a failed tab — new attempt, same tab. */
  const retryConnect = (key: string) => {
    const tab = tabsRef.current.find((t) => t.sessionId === key);
    if (!tab || tab.phase !== "failed") return;
    const attemptId = crypto.randomUUID();
    setTabs((prev) =>
      prev.map((t) =>
        t.sessionId === key
          ? { ...t, phase: "connecting", attemptId, progress: [], failure: null }
          : t,
      ),
    );
    // Make the ref current right away so the attempt owns the tab.
    tabsRef.current = tabsRef.current.map((t) =>
      t.sessionId === key ? { ...t, phase: "connecting", attemptId } : t,
    );
    void runAttempt(key, tab.serverId, attemptId, tab.resumeChatSessionId);
  };

  /** Spec 0104: nur Nutzer-Tabs — ein MCP-Tab desselben Servers ist nie
   * der Tab des Nutzers für diesen Server. */
  const findExistingSessionId = (serverId: string): string | undefined =>
    // Issue #657: with several tabs, the most recently used one.
    mostRecentlyUsed(
      tabs.filter((t) => t.serverId === serverId && !t.mcp),
      mruRef.current,
    )?.sessionId;

  const switchTo = (sessionId: string | null) => setActiveSessionId(sessionId);

  /** Vom Chat-Bestätigungsdialog aufgerufen (`respond`/`acceptWithRule` in
   * `ChatPanel`), sobald der Nutzer eine wartende Aktion auflöst — nötig,
   * weil eine Ablehnung (anders als Approve/EditThenApprove) kein
   * `chat-action-result`-Event auslöst (s. `ChatPanel`-Doc-Kommentar), der
   * Indikator hier also sonst hängen bliebe. */
  const markActionSettled = (sessionId: string) => {
    setTabs((prev) =>
      prev.map((t) =>
        t.sessionId === sessionId ? { ...t, hasPendingAction: false, pendingActionId: null } : t,
      ),
    );
  };

  const removeTabAndPickNextActive = (sessionId: string) => {
    setTabs((prev) => prev.filter((t) => t.sessionId !== sessionId));
    setActiveSessionId((prevActive) => {
      if (prevActive !== sessionId) return prevActive;
      // Aus dem aktuellen Stand (`tabsRef`), nicht aus `tabs` der Closure:
      // `requestCloseTab` wartet vorher auf `disconnect`, in der Zwischenzeit
      // können Tabs dazugekommen oder geschlossen worden sein.
      return nextActiveAfterClose(tabsRef.current, sessionId);
    });
  };

  /** Spec 0017, Abschnitt 6: Schließen-Button/`Cmd`/`Ctrl+W` rufen
   * `disconnect(session_id)` auf. Abschnitt 5, letzter Punkt: steht eine
   * Bestätigung aus, erst eine Rückfrage — bei Bestätigung des Schließens
   * gilt die wartende Aktion als **abgelehnt**, nicht als genehmigt. */
  const requestCloseTab = async (sessionId: string) => {
    const tab = tabs.find((t) => t.sessionId === sessionId);
    if (tab && tab.phase !== "connected") {
      // Issue #548: a connecting tab cancels its attempt (no error shown);
      // a failed tab has nothing to tear down.
      const attemptId = tab.attemptId;
      removeTabAndPickNextActive(sessionId);
      if (attemptId) {
        try {
          await cancelConnect(attemptId);
        } catch (err) {
          console.error(commandErrorMessage(err));
        }
      }
      return;
    }
    if (tab?.hasPendingAction) {
      const proceed = window.confirm(
        translate("sessionTabs.confirmClosePending", { name: tab.serverName }),
      );
      if (!proceed) return;
      if (tab.pendingActionId) {
        try {
          await respondToAction(sessionId, tab.pendingActionId, { decision: "deny" });
        } catch (err) {
          // Best-effort: die Aktion wurde evtl. inzwischen anderweitig
          // aufgelöst (z. B. Sender bereits weg) — das Schließen des Tabs
          // soll daran nicht scheitern.
          console.error(commandErrorMessage(err));
        }
      }
    }
    try {
      await disconnect(sessionId);
    } catch (err) {
      console.error(commandErrorMessage(err));
    }
    forgetChatSessionId(sessionId);
    removeTabAndPickNextActive(sessionId);
  };

  return {
    tabs,
    activeSessionId,
    openTab,
    startConnect,
    retryConnect,
    findExistingSessionId,
    switchTo,
    markActionSettled,
    requestCloseTab,
  };
}
