import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode, commandErrorMessage, reconnectSession, stopAutoReconnect } from "../api";
import { translateErrorCode } from "../errorCodes";
import { onAutoReconnectChanged, onConnectionStatusChanged } from "../events";
import type { McpSessionInfo } from "../types";
import { sessionTabLabel } from "../useSessionTabs";
import { loadAiSshSplitWidthPx, saveAiSshSplitWidthPx } from "../layoutSettings";
import { useDragResize } from "../useDragResize";
import { ChatPanel } from "./ChatPanel";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { Button, Tabs, useTabsIdBase, type TabItem } from "./ui";
import { panelId, tabId } from "./ui/Tabs";
import { FileBrowserPanel } from "./FileBrowserPanel";
import { TerminalView, type TerminalLink } from "./TerminalView";

/** Spec 0053, Teil 2: Mindestbreiten beider Bereiche (px) — keiner darf auf
 * 0 gezogen werden. `RIGHT_PANEL_DEFAULT_WIDTH` ist die bisherige feste
 * Breite (`w-[420px]`), jetzt nur noch der Startwert vor dem ersten Laden
 * einer Präferenz bzw. der Fallback auf zu kleinen Fenstern. */
const LEFT_PANEL_MIN_WIDTH = 360;
const RIGHT_PANEL_MIN_WIDTH = 300;
const RIGHT_PANEL_DEFAULT_WIDTH = 420;
const SPLIT_HANDLE_WIDTH = 6;

/** Fenster zu schmal, um beide Mindestbreiten gleichzeitig unterzubringen. */
function windowTooSmallForBothPanels(containerWidth: number): boolean {
  return containerWidth < LEFT_PANEL_MIN_WIDTH + RIGHT_PANEL_MIN_WIDTH + SPLIT_HANDLE_WIDTH;
}

/** Klemmt die *angezeigte* Breite des rechten (SSH-/SFTP-)Bereichs auf
 * seine Mindestbreite UND darauf, dass dem linken (KI-)Bereich mindestens
 * `LEFT_PANEL_MIN_WIDTH` bleibt. Passt das Fenster nicht einmal für beide
 * Mindestbreiten zusammen (Spec 0053, Teil 2: "auf kleinen Fenstern...
 * notfalls Fallback auf die Standardaufteilung"), wird auf
 * `RIGHT_PANEL_DEFAULT_WIDTH` zurückgefallen statt einen widersprüchlichen
 * geklemmten Zustand zu erzwingen. Nur fürs *Rendern* (`effectiveRightWidth`)
 * gedacht — s. `clampRightPanelWidthForDrag` für die Geste selbst. */
export function clampRightPanelWidthForDisplay(proposed: number, containerWidth: number): number {
  if (windowTooSmallForBothPanels(containerWidth)) {
    // Issue #580: never wider than the window — each pane gets at most half.
    return Math.max(
      0,
      Math.min(RIGHT_PANEL_DEFAULT_WIDTH, Math.floor((containerWidth - SPLIT_HANDLE_WIDTH) / 2)),
    );
  }
  const maxRightWidth = containerWidth - LEFT_PANEL_MIN_WIDTH - SPLIT_HANDLE_WIDTH;
  return Math.min(Math.max(proposed, RIGHT_PANEL_MIN_WIDTH), maxRightWidth);
}

/** Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): `preferredRightWidth`
 * darf NIE auf `RIGHT_PANEL_DEFAULT_WIDTH` gesetzt werden, nur weil das
 * Fenster gerade zu klein ist — genau das tat der vorherige, gemeinsam mit
 * `clampRightPanelWidthForDisplay` genutzte Klemm-Code, sobald während
 * einer Ziehgeste im zu kleinen Fenster `preferredRightWidth` selbst
 * (nicht nur die Anzeige) auf 420 gesetzt und anschließend auch noch
 * persistiert wurde — eine gespeicherte Präferenz (z. B. 900px) ging durch
 * ein einziges Ruckeln am Divider in einem schmalen Fenster dauerhaft
 * verloren. Diese Variante lässt die Geste im zu-klein-Regime stattdessen
 * wirkungslos verpuffen (`prev` unverändert), statt die Präferenz
 * stillschweigend zu überschreiben. */
function clampRightPanelWidthForDrag(proposed: number, containerWidth: number, prev: number): number {
  if (windowTooSmallForBothPanels(containerWidth)) {
    return prev;
  }
  const maxRightWidth = containerWidth - LEFT_PANEL_MIN_WIDTH - SPLIT_HANDLE_WIDTH;
  return Math.min(Math.max(proposed, RIGHT_PANEL_MIN_WIDTH), maxRightWidth);
}

type NarrowTab = "ai" | "terminal" | "files";

interface SessionViewProps {
  sessionId: string;
  serverName: string;
  serverId: string;
  /** Spec 0017, Abschnitt 6: `disconnect(session_id)` läuft jetzt zentral in
   * `useSessionTabs.requestCloseTab` (prüft zuerst auf eine wartende
   * Bestätigung, Abschnitt 5, letzter Punkt) — `SessionView` selbst ruft
   * `disconnect()` nicht mehr direkt auf, das "Trennen"-Element im Header
   * unten löst denselben zentralen Fluss aus wie der Schließen-Button in
   * der Tab-Leiste. */
  onRequestClose: () => void;
  /** Spec 0017, Abschnitt 5 — an `ChatPanel` durchgereicht, s. dortiger
   * Doc-Kommentar. */
  onActionSettled: (sessionId: string) => void;
  /** Spec 0020, Abschnitt 5.4: nur der aktive Tab darf auf einen
   * OS-Drag-and-Drop-Upload reagieren, s. `FileBrowserPanel.isVisible`-Doc-
   * Kommentar — jede offene Session bleibt beim Tab-Wechsel gemountet (Spec
   * 0017, Abschnitt 4), ohne dieses Flag würde ein Drop sonst gleichzeitig
   * mehrere Hintergrund-Tabs als Ziel treffen. */
  isActiveTab: boolean;
  /** Spec 0104: `null` für eine Nutzer-Sitzung. Eine MCP-Sitzung zeigt nur
   * die Aktionskarten des externen Clients (Bestätigen/Ablehnen) samt
   * Ergebnis — ohne Chat-Eingabe, Terminal und Dateibrowser. */
  mcp?: McpSessionInfo | null;
}

/**
 * Spec 0007 Abschnitt 7: Chat-Panel groß links (primärer Interaktionskanal),
 * Terminal kompakt rechts (Beobachtung/manuelle Zwischen-Eingriffe), sobald
 * eine Session steht.
 */
export function SessionView({
  sessionId,
  serverName,
  serverId,
  onRequestClose,
  onActionSettled,
  isActiveTab,
  mcp = null,
}: SessionViewProps) {
  const { t } = useTranslation();
  /** Grund des Verbindungsabbruchs; `null` = noch verbunden. Gespeichert
   * wird der Grund statt des fertigen Texts, damit ein Sprachwechsel auch
   * diesen Hinweis sofort umstellt (Spec 0024, Abschnitt 4). */
  const [disconnectReason, setDisconnectReason] = useState<{ reason: string | null } | null>(null);
  // Issue #355: Zustand der Verbindung dieses Tabs — `lost` zeigt den
  // "Neu verbinden"-Knopf, `reconnecting` sperrt ihn.
  const [link, setLink] = useState<TerminalLink>("live");
  const [reconnectError, setReconnectError] = useState<string | null>(null);
  const reconnectBusy = useRef(false);
  // Issue #356: automatische Neuverbindungsversuche — `attempt` ist die
  // Nummer des laufenden bzw. nächsten Versuchs, `null` = keine laufen;
  // `gaveUp` = die Zeit ist abgelaufen.
  const [autoAttempt, setAutoAttempt] = useState<number | null>(null);
  const [gaveUp, setGaveUp] = useState(false);
  const autoActive = useRef(false);
  // Spec 0020, Abschnitt 5.1: "Terminal | Dateien"-Umschalter im rechten
  // Panel. Beide Ansichten bleiben gemountet (analog zum
  // Immer-gemountet-Muster der Session-Tabs selbst, Spec 0017 Abschnitt 4) —
  // nur per CSS ausgeblendet, damit weder xterm-Scrollback noch die aktuelle
  // Verzeichnisnavigation des Dateibrowsers beim Umschalten verloren gehen.
  const [rightPanelView, setRightPanelView] = useState<"terminal" | "files">("terminal");
  // Spec 0067, A5: Ziel-Nutzer des erhöhten Dateibrowser-Modus, `null` = aus.
  // Issue #580, Spec 0113 §4: below 640 px one pane at a time; AI is active
  // after connecting. Kept per session (this component lives per session).
  const narrow = useIsNarrow();
  const [narrowTab, setNarrowTab] = useState<NarrowTab>("ai");
  const paneTabsBase = useTabsIdBase();
  const [menuOpen, setMenuOpen] = useState(false);
  const [filesElevatedUser, setFilesElevatedUser] = useState<string | null>(null);

  // Spec 0053, Teil 2: `preferredRightWidth` ist die vom Nutzer gewählte
  // (bzw. geladene) Breite — wird NIE durch ein zu kleines Fenster
  // stillschweigend überschrieben, nur `effectiveRightWidth` (unten) klemmt
  // sie zur Anzeige. Andernfalls würde ein zwischenzeitlich verkleinertes
  // Fenster die eigentliche Präferenz dauerhaft verlieren, sobald das
  // Fenster wieder vergrößert wird.
  const [preferredRightWidth, setPreferredRightWidth] = useState(RIGHT_PANEL_DEFAULT_WIDTH);
  const [containerWidth, setContainerWidth] = useState<number | null>(null);
  const splitContainerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    loadAiSshSplitWidthPx()
      .then((stored) => {
        if (stored !== null) setPreferredRightWidth(stored);
      })
      .catch((err) => console.warn("Konnte Bereichsaufteilung nicht laden:", err));
  }, []);

  // Reagiert auf jede Größenänderung des umgebenden Containers (nicht nur
  // auf Drag-Gesten) — ein Fenster, das der Nutzer nach dem letzten Ziehen
  // kleiner macht, darf das Layout nicht unbrauchbar machen (Spec 0053,
  // Teil 2).
  useEffect(() => {
    const el = splitContainerRef.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) setContainerWidth(entry.contentRect.width);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const effectiveRightWidth =
    containerWidth === null
      ? preferredRightWidth
      : clampRightPanelWidthForDisplay(preferredRightWidth, containerWidth);

  // Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): derselbe
  // StrictMode-Seiteneffekt-im-Updater-Befund wie in
  // `FileBrowserPanel.tsx` — `preferredRightWidthRef` hält den Wert
  // synchron nach, `handleSplitDragEnd` liest ihn direkt.
  const preferredRightWidthRef = useRef(preferredRightWidth);
  useEffect(() => {
    preferredRightWidthRef.current = preferredRightWidth;
  }, [preferredRightWidth]);

  const handleSplitDrag = useCallback((deltaX: number) => {
    setPreferredRightWidth((prev) => {
      const containerWidthNow = splitContainerRef.current?.clientWidth ?? Infinity;
      // Divider nach links gezogen (negatives Delta) vergrößert den
      // rechten Bereich, daher `prev - deltaX`.
      return clampRightPanelWidthForDrag(prev - deltaX, containerWidthNow, prev);
    });
  }, []);

  const handleSplitDragEnd = useCallback(() => {
    saveAiSshSplitWidthPx(preferredRightWidthRef.current).catch((err) =>
      console.warn("Konnte Bereichsaufteilung nicht speichern:", err),
    );
  }, []);

  const splitHandlers = useDragResize(handleSplitDrag, handleSplitDragEnd);

  const statusNote =
    disconnectReason === null
      ? null
      : disconnectReason.reason
        ? t("session.disconnectedWithReason", { reason: disconnectReason.reason })
        : t("session.disconnected");

  useEffect(() => {
    const unlisten = onConnectionStatusChanged((event) => {
      if (event.sessionId !== sessionId) return;
      if (event.status === "disconnected") {
        // Ein fehlgeschlagener Reconnect meldet `disconnected` ohne Grund;
        // der ursprüngliche Grund bleibt dann stehen.
        setDisconnectReason((prev) => ({ reason: event.reason ?? prev?.reason ?? null }));
        setLink("lost");
      } else if (event.status === "reconnecting") {
        setLink("reconnecting");
      } else if (event.status === "connected") {
        setDisconnectReason(null);
        setReconnectError(null);
        setGaveUp(false);
        setLink("live");
      }
    });
    return () => {
      unlisten.then((unlistenFn) => unlistenFn());
    };
  }, [sessionId]);

  // Issue #356: Stand der automatischen Neuverbindung. Das Backend meldet
  // `running` vor dem Verbindungsabbruch-Ereignis, die Anzeige springt also
  // nie kurz auf "verloren".
  useEffect(() => {
    const unlisten = onAutoReconnectChanged((event) => {
      if (event.sessionId !== sessionId) return;
      if (event.phase === "running") {
        autoActive.current = true;
        setGaveUp(false);
        setAutoAttempt(event.attempt);
        return;
      }
      autoActive.current = false;
      setAutoAttempt(null);
      setGaveUp(event.phase === "gave_up");
      // Ein selbst gestarteter Versuch (Knopf) hat die Anzeige schon.
      if (event.phase !== "reconnected" && !reconnectBusy.current) setLink("lost");
    });
    return () => {
      unlisten.then((unlistenFn) => unlistenFn());
    };
  }, [sessionId]);

  const handleStopAutoReconnect = useCallback(() => {
    stopAutoReconnect(sessionId).catch((err) =>
      setReconnectError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err))),
    );
  }, [sessionId, t]);

  // Issue #355: Knopf und Taste R lösen denselben Pfad aus; `reconnectBusy`
  // fängt einen zweiten Auslöser ab, bevor React neu gerendert hat.
  const handleReconnect = useCallback(() => {
    if (reconnectBusy.current || autoActive.current) return;
    reconnectBusy.current = true;
    setReconnectError(null);
    setGaveUp(false);
    setLink("reconnecting");
    reconnectSession(sessionId)
      .then(() => {
        setDisconnectReason(null);
        setLink("live");
      })
      .catch((err) => {
        setReconnectError(
          translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)),
        );
        setLink("lost");
      })
      .finally(() => {
        reconnectBusy.current = false;
      });
  }, [sessionId, t]);

  // Spec 0104, §4: MCP-Sitzung — nur-lesende Sicht auf das, was der
  // externe Client angefragt hat und was davon lief. Kein Terminal (das
  // Backend lehnt `open_terminal` für MCP-Sitzungen ohnehin ab) und keine
  // Chat-Eingabe (dito `send_chat_message`).
  if (mcp) {
    return (
      <div className="flex flex-1 min-h-0 flex-col bg-slate-900 text-slate-100">
        <header className="flex items-center justify-between gap-3 border-b border-fuchsia-900/60 px-4 py-2">
          <div className="flex min-w-0 items-center gap-3">
            <span className="shrink-0 border border-fuchsia-500/60 px-1.5 text-xs text-fuchsia-300">
              {t("sessionTabs.mcp.badge")}
            </span>
            <span
              title={sessionTabLabel({ serverName, mcp }, t("sessionTabs.mcp.unnamedClient"))}
              className="font-heading min-w-0 truncate font-semibold tracking-wide"
            >
              {sessionTabLabel({ serverName, mcp }, t("sessionTabs.mcp.unnamedClient"))}
            </span>
            {statusNote && <span className="font-mono text-xs text-amber-300">{statusNote}</span>}
          </div>
          <Button onClick={onRequestClose} className="shrink-0">
            {t("session.disconnect")}
          </Button>
        </header>
        <div className="min-h-0 flex-1">
          <ChatPanel
            sessionId={sessionId}
            serverId={serverId}
            onActionSettled={onActionSettled}
            readOnlyHint={t("sessionTabs.mcp.readOnlyHint")}
          />
        </div>
      </div>
    );
  }

  const selectNarrowTab = (id: string) => {
    const tab = id as NarrowTab;
    setNarrowTab(tab);
    // Keep the wide right-panel choice in step with the last Terminal/Files tab.
    if (tab !== "ai") setRightPanelView(tab);
  };
  const narrowTabs: TabItem[] = [
    { id: "ai", label: t("session.aiTab") },
    { id: "terminal", label: t("session.terminalTab") },
    {
      id: "files",
      label: (
        <>
          {t("session.filesTab")}
          {filesElevatedUser && <span className="ml-1 text-amber-400">⚠</span>}
        </>
      ),
    },
  ];
  const showTerminal = narrow ? narrowTab === "terminal" : rightPanelView === "terminal";
  const showFiles = narrow ? narrowTab === "files" : rightPanelView === "files";
  const panelAria = (id: NarrowTab) =>
    narrow
      ? ({ role: "tabpanel", id: panelId(paneTabsBase, id), "aria-labelledby": tabId(paneTabsBase, id) } as const)
      : {};

  const showReconnect = link !== "live" && autoAttempt === null;
  const reconnectButton = (
    <Button
      variant="secondary"
      size="sm"
      onClick={() => {
        setMenuOpen(false);
        handleReconnect();
      }}
      disabled={link === "reconnecting"}
      className={narrow ? "min-h-11 w-full" : "shrink-0"}
    >
      {link === "reconnecting" ? t("session.reconnecting") : t("session.reconnect")}
    </Button>
  );

  return (
    <div className="flex flex-1 min-h-0 flex-col bg-slate-900 text-slate-100">
      <header className="relative flex items-center justify-between gap-3 border-b border-slate-800 px-4 py-2">
        <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
          <span title={serverName} className="font-heading min-w-0 truncate font-semibold tracking-wide">
            {serverName}
          </span>
          {statusNote && <span className="font-mono text-xs text-amber-300">{statusNote}</span>}
          {autoAttempt !== null && (
            <>
              <output className="font-mono text-xs text-amber-300">
                {t("session.reconnectingAuto", { attempt: autoAttempt })}
              </output>
              <button
                type="button"
                onClick={handleStopAutoReconnect}
                className={`shrink-0 text-xs text-indigo-300 underline hover:text-indigo-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 ${narrow ? "min-h-11 px-2" : ""}`}
              >
                {t("session.stopReconnect")}
              </button>
            </>
          )}
          {gaveUp && autoAttempt === null && (
            <output className="font-mono text-xs text-amber-300">
              {t("session.reconnectGaveUp")}
            </output>
          )}
          {!narrow && showReconnect && reconnectButton}
          {reconnectError && (
            <span role="alert" className="font-mono text-xs text-red-300">
              {reconnectError}
            </span>
          )}
        </div>
        {narrow ? (
          <>
            <Button
              variant="secondary"
              onClick={() => setMenuOpen((open) => !open)}
              aria-expanded={menuOpen}
              aria-label={t("session.moreActions")}
              className="min-h-11 min-w-11 shrink-0"
            >
              ⋯
            </Button>
            {menuOpen && (
              <div
                onKeyDown={(e) => {
                  if (e.key === "Escape") setMenuOpen(false);
                }}
                className="absolute right-2 top-full z-20 flex w-48 max-w-[calc(100vw-1rem)] flex-col gap-2 rounded border border-slate-700 bg-slate-900 p-2 shadow-lg"
              >
                {showReconnect && reconnectButton}
                <Button
                  className="min-h-11 w-full"
                  onClick={() => {
                    setMenuOpen(false);
                    onRequestClose();
                  }}
                >
                  {t("session.disconnect")}
                </Button>
              </div>
            )}
          </>
        ) : (
          <Button onClick={onRequestClose} className="shrink-0">
            {t("session.disconnect")}
          </Button>
        )}
      </header>
      {narrow && (
        <Tabs
          tabs={narrowTabs}
          value={narrowTab}
          onChange={selectNarrowTab}
          ariaLabel={t("session.paneTabs")}
          idBase={paneTabsBase}
          tabClassName="min-h-11 flex-1"
        />
      )}
      <div ref={splitContainerRef} className={narrow ? "flex min-h-0 flex-1 flex-col" : "flex min-h-0 flex-1"}>
        <div
          {...panelAria("ai")}
          className={narrow ? (narrowTab === "ai" ? "min-h-0 min-w-0 flex-1" : "hidden") : "min-w-0 flex-1"}
        >
          <ChatPanel sessionId={sessionId} serverId={serverId} onActionSettled={onActionSettled} />
        </div>
        {/* Spec 0053, Teil 2: Drag-Divider zwischen KI- und SSH-/SFTP-
            Bereich — ersetzt den vorherigen statischen `border-r`. Im
            schmalen Layout gibt es ihn nicht (Issue #580). */}
        {!narrow && (
          <span
            // eslint-disable-next-line jsx-a11y/prefer-tag-over-role -- ziehbarer Trenner mit Griff-Kindelement; <hr> ist ein leeres Element und kann den Griff nicht aufnehmen (Issue #112).
            role="separator"
            aria-orientation="vertical"
            aria-label={t("session.splitHandle")}
            className="group relative w-1.5 shrink-0 cursor-col-resize touch-none select-none bg-slate-800"
            {...splitHandlers}
          >
            <span className="absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-slate-700 group-hover:bg-indigo-500" />
          </span>
        )}
        <div
          className={
            narrow
              ? narrowTab === "ai"
                ? "hidden"
                : "flex min-h-0 min-w-0 flex-1 flex-col bg-slate-950"
              : "flex shrink-0 flex-col bg-slate-950"
          }
          style={narrow ? undefined : { width: effectiveRightWidth }}
        >
          {!narrow && (
            <div className="flex h-8 shrink-0 items-center gap-3 border-b border-slate-800 px-3">
              <button
                type="button"
                onClick={() => setRightPanelView("terminal")}
                aria-pressed={rightPanelView === "terminal"}
                className={`font-heading text-xs font-semibold tracking-[0.13em] uppercase focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 ${
                  rightPanelView === "terminal" ? "text-indigo-400" : "text-slate-400 hover:text-slate-200"
                }`}
              >
                {t("session.terminalTab")}
              </button>
              <button
                type="button"
                onClick={() => setRightPanelView("files")}
                aria-pressed={rightPanelView === "files"}
                className={`font-heading text-xs font-semibold tracking-[0.13em] uppercase focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 ${
                  rightPanelView === "files" ? "text-indigo-400" : "text-slate-400 hover:text-slate-200"
                }`}
              >
                {t("session.filesTab")}
                {filesElevatedUser && (
                  // Spec 0067, A5: erhöhter Modus bleibt auch bei verborgener
                  // Dateien-Ansicht sichtbar.
                  <span className="ml-1.5 normal-case tracking-normal text-amber-400">
                    ⚠ {filesElevatedUser}
                  </span>
                )}
              </button>
            </div>
          )}
          <div {...panelAria("terminal")} className={showTerminal ? "min-h-0 flex-1 p-2" : "hidden"}>
            <TerminalView
              sessionId={sessionId}
              link={autoAttempt !== null ? "reconnecting" : link}
              onReconnect={handleReconnect}
            />
          </div>
          <div {...panelAria("files")} className={showFiles ? "min-h-0 flex-1" : "hidden"}>
            <FileBrowserPanel
              sessionId={sessionId}
              isVisible={isActiveTab && showFiles}
              onElevationChange={setFilesElevatedUser}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
