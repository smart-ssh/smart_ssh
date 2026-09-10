import { useCallback, useEffect, useRef, useState } from "react";
import { onConnectionStatusChanged } from "../events";
import { loadAiSshSplitWidthPx, saveAiSshSplitWidthPx } from "../layoutSettings";
import { useDragResize } from "../useDragResize";
import { ChatPanel } from "./ChatPanel";
import { FileBrowserPanel } from "./FileBrowserPanel";
import { TerminalView } from "./TerminalView";

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
function clampRightPanelWidthForDisplay(proposed: number, containerWidth: number): number {
  if (windowTooSmallForBothPanels(containerWidth)) {
    return RIGHT_PANEL_DEFAULT_WIDTH;
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
}: SessionViewProps) {
  const [statusNote, setStatusNote] = useState<string | null>(null);
  // Spec 0020, Abschnitt 5.1: "Terminal | Dateien"-Umschalter im rechten
  // Panel. Beide Ansichten bleiben gemountet (analog zum
  // Immer-gemountet-Muster der Session-Tabs selbst, Spec 0017 Abschnitt 4) —
  // nur per CSS ausgeblendet, damit weder xterm-Scrollback noch die aktuelle
  // Verzeichnisnavigation des Dateibrowsers beim Umschalten verloren gehen.
  const [rightPanelView, setRightPanelView] = useState<"terminal" | "files">("terminal");

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

  useEffect(() => {
    const unlisten = onConnectionStatusChanged((event) => {
      if (event.sessionId !== sessionId) return;
      if (event.status === "disconnected") {
        setStatusNote(event.reason ? `Verbindung getrennt: ${event.reason}` : "Verbindung getrennt");
      }
    });
    return () => {
      unlisten.then((unlistenFn) => unlistenFn());
    };
  }, [sessionId]);

  return (
    <div className="flex flex-1 min-h-0 flex-col bg-slate-900 text-slate-100">
      <header className="flex items-center justify-between border-b border-slate-800 px-4 py-2">
        <div className="flex items-center gap-3">
          <span className="font-heading font-semibold tracking-wide">{serverName}</span>
          {statusNote && <span className="font-mono text-xs text-amber-300">{statusNote}</span>}
        </div>
        <button
          type="button"
          onClick={onRequestClose}
          className="font-heading border border-slate-700 px-3 py-1.5 text-sm font-semibold tracking-wide text-slate-200 hover:bg-slate-800"
        >
          Trennen
        </button>
      </header>
      <div ref={splitContainerRef} className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1">
          <ChatPanel sessionId={sessionId} serverId={serverId} onActionSettled={onActionSettled} />
        </div>
        {/* Spec 0053, Teil 2: Drag-Divider zwischen KI- und SSH-/SFTP-
            Bereich — ersetzt den vorherigen statischen `border-r`. */}
        <span
          role="separator"
          aria-orientation="vertical"
          aria-label="Bereichsaufteilung"
          className="group relative w-1.5 shrink-0 cursor-col-resize touch-none select-none bg-slate-800"
          {...splitHandlers}
        >
          <span className="absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-slate-700 group-hover:bg-indigo-500" />
        </span>
        <div
          className="flex shrink-0 flex-col bg-slate-950"
          style={{ width: effectiveRightWidth }}
        >
          <div className="flex h-8 shrink-0 items-center gap-3 border-b border-slate-800 px-3">
            <button
              type="button"
              onClick={() => setRightPanelView("terminal")}
              className={`font-heading text-xs font-semibold tracking-[0.13em] uppercase ${
                rightPanelView === "terminal" ? "text-indigo-400" : "text-slate-500 hover:text-slate-300"
              }`}
            >
              Terminal
            </button>
            <button
              type="button"
              onClick={() => setRightPanelView("files")}
              className={`font-heading text-xs font-semibold tracking-[0.13em] uppercase ${
                rightPanelView === "files" ? "text-indigo-400" : "text-slate-500 hover:text-slate-300"
              }`}
            >
              Dateien
            </button>
          </div>
          <div className={rightPanelView === "terminal" ? "min-h-0 flex-1 p-2" : "hidden"}>
            <TerminalView sessionId={sessionId} />
          </div>
          <div className={rightPanelView === "files" ? "min-h-0 flex-1" : "hidden"}>
            <FileBrowserPanel
              sessionId={sessionId}
              isVisible={isActiveTab && rightPanelView === "files"}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
