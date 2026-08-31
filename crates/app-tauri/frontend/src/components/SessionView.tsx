import { useEffect, useState } from "react";
import { commandErrorMessage, disconnect } from "../api";
import { onConnectionStatusChanged } from "../events";
import { ChatPanel } from "./ChatPanel";
import { TerminalView } from "./TerminalView";

interface SessionViewProps {
  sessionId: string;
  serverName: string;
  serverId: string;
  onDisconnected: () => void;
}

/**
 * Spec 0007 Abschnitt 7: Chat-Panel groß links (primärer Interaktionskanal),
 * Terminal kompakt rechts (Beobachtung/manuelle Zwischen-Eingriffe), sobald
 * eine Session steht.
 */
export function SessionView({ sessionId, serverName, serverId, onDisconnected }: SessionViewProps) {
  const [statusNote, setStatusNote] = useState<string | null>(null);

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

  const handleDisconnect = async () => {
    try {
      await disconnect(sessionId);
    } catch (err) {
      console.error(commandErrorMessage(err));
    } finally {
      onDisconnected();
    }
  };

  return (
    <div className="flex flex-1 min-h-0 flex-col bg-slate-900 text-slate-100">
      <header className="flex items-center justify-between border-b border-slate-800 px-4 py-2">
        <div>
          <span className="font-medium">{serverName}</span>
          {statusNote && <span className="ml-3 text-xs text-amber-300">{statusNote}</span>}
        </div>
        <button
          type="button"
          onClick={handleDisconnect}
          className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700"
        >
          Trennen
        </button>
      </header>
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1 border-r border-slate-800">
          <ChatPanel sessionId={sessionId} serverId={serverId} />
        </div>
        <div className="w-[420px] shrink-0 p-2">
          <TerminalView sessionId={sessionId} />
        </div>
      </div>
    </div>
  );
}
