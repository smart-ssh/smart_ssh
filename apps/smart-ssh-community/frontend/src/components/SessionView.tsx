import { useEffect, useState } from "react";
import { onConnectionStatusChanged } from "../events";
import { ChatPanel } from "./ChatPanel";
import { FileBrowserPanel } from "./FileBrowserPanel";
import { TerminalView } from "./TerminalView";

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
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1 border-r border-slate-800">
          <ChatPanel sessionId={sessionId} serverId={serverId} onActionSettled={onActionSettled} />
        </div>
        <div className="flex w-[420px] shrink-0 flex-col bg-slate-950">
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
