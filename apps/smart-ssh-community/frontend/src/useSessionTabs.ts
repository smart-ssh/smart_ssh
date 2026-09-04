// Spec 0017: zentrale Buchführung über offene Session-Tabs. Bewusst
// getrennt vom eigentlichen Chat-/Terminal-Zustand (der bleibt lokal in
// `ChatPanel`/`TerminalView`, s. dortige Doc-Kommentare) — dieser Hook hält
// nur, was die Tab-Leiste selbst zum Rendern braucht: welche Sessions
// offen sind, ihr Servername/Status, und ob eine Bestätigung aussteht
// (Abschnitt 5, Tab-Indikator). Jede Zuordnung hier erfolgt strikt über
// `event.sessionId` aus den jeweiligen Events, nie implizit über den
// gerade aktiven Tab (Abschnitt 4, "das ist der wahrscheinlichste
// Fehlerfall bei dieser Umstellung").
import { useEffect, useState } from "react";
import { commandErrorMessage, disconnect, getServer, listSessions, respondToAction } from "./api";
import {
  onChatActionProposed,
  onChatActionResult,
  onConnectionStatusChanged,
  onMcpActionTabRequested,
} from "./events";
import type { ConnectionStatus } from "./types";

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
}

export function useSessionTabs() {
  const [tabs, setTabs] = useState<SessionTab[]>([]);
  // `null` heißt "Übersicht" (Server-/Verwaltungs-Screens) ist aktiv, nicht
  // "keine Tabs offen" — beides ist gleichzeitig möglich (Tabs bleiben im
  // Hintergrund offen, während der Nutzer einen weiteren Server sucht).
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);

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
          })),
        );
        setActiveSessionId((prev) => prev ?? summaries[0].sessionId);
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
            },
          ];
        });
        setActiveSessionId(event.sessionId);
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
        },
      ];
    });
    setActiveSessionId(sessionId);
  };

  const findExistingSessionId = (serverId: string): string | undefined =>
    tabs.find((t) => t.serverId === serverId)?.sessionId;

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
      const remaining = tabs.filter((t) => t.sessionId !== sessionId);
      return remaining.length > 0 ? remaining[remaining.length - 1].sessionId : null;
    });
  };

  /** Spec 0017, Abschnitt 6: Schließen-Button/`Cmd`/`Ctrl+W` rufen
   * `disconnect(session_id)` auf. Abschnitt 5, letzter Punkt: steht eine
   * Bestätigung aus, erst eine Rückfrage — bei Bestätigung des Schließens
   * gilt die wartende Aktion als **abgelehnt**, nicht als genehmigt. */
  const requestCloseTab = async (sessionId: string) => {
    const tab = tabs.find((t) => t.sessionId === sessionId);
    if (tab?.hasPendingAction) {
      const proceed = window.confirm(
        `Für "${tab.serverName}" wartet noch eine Bestätigung. Tab wirklich schließen? ` +
          "Die wartende Aktion gilt dann als abgelehnt.",
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
    findExistingSessionId,
    switchTo,
    markActionSettled,
    requestCloseTab,
  };
}
