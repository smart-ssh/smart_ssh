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
} from "./api";
import { translateErrorCode } from "./errorCodes";
import {
  onActionDecisionEscalated,
  onChatActionProposed,
  onChatActionResult,
  onConnectProgress,
  onConnectionStatusChanged,
  onMcpActionTabRequested,
} from "./events";
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
  /** Issue #548. */
  phase: TabPhase;
  /** Issue #548: id of the running connect attempt (`connecting` only). */
  attemptId: string | null;
  /** Issue #548: live step list from `connect-progress`. */
  progress: ConnectStepSnapshot[];
  /** Issue #548: set in phase `failed`. */
  failure: ConnectFailure | null;
}

/** Spec 0104: Beschriftung "<Client> @ <Server>" für MCP-Tabs. */
export function sessionTabLabel(
  tab: Pick<SessionTab, "serverName" | "mcp">,
  unnamedClientLabel: string,
): string {
  if (!tab.mcp) return tab.serverName;
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

  // Spec 0017, Abschnitt 2: Backend ist die maßgebliche Quelle offener
  // Sessions — beim Start (bzw. Dev-Modus-Hot-Reload) wird die Tab-Leiste
  // daraus wiederhergestellt statt von einem leeren Zustand auszugehen.
  useEffect(() => {
    listSessions()
      .then((summaries) => {
        if (summaries.length === 0) return;
        setTabs(
          summaries.map((s) => ({
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
            ...CONNECTED_FIELDS,
          })),
        );
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
          ...CONNECTED_FIELDS,
        },
      ];
    });
    setActiveSessionId(sessionId);
  };

  /** Issue #548: runs one connect attempt for the tab `key`. The tab turns
   * into the session tab in place on success, shows the failure on error and
   * is left alone if the attempt was cancelled / the tab closed meanwhile. */
  const runAttempt = async (key: string, serverId: string, attemptId: string) => {
    const stillOwnsTab = () =>
      tabsRef.current.some((t) => t.sessionId === key && t.attemptId === attemptId);
    try {
      const sessionId = await connect(serverId, attemptId);
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
  const startConnect = (server: { id: string; name: string }) => {
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
      phase: "connecting",
      attemptId,
      progress: [],
      failure: null,
    };
    setTabs((prev) => [...prev, tab]);
    // `tabsRef` is otherwise refreshed only after a render; `runAttempt`
    // may be resumed before that.
    tabsRef.current = [...tabsRef.current, tab];
    setActiveSessionId(key);
    void runAttempt(key, server.id, attemptId);
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
    void runAttempt(key, tab.serverId, attemptId);
  };

  /** Spec 0104: nur Nutzer-Tabs — ein MCP-Tab desselben Servers ist nie
   * der Tab des Nutzers für diesen Server. */
  const findExistingSessionId = (serverId: string): string | undefined =>
    tabs.find((t) => t.serverId === serverId && !t.mcp)?.sessionId;

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
