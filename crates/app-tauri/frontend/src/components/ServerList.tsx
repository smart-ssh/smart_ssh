import { useEffect, useState } from "react";
import { commandErrorMessage, confirmHostKey, connect, listServers } from "../api";
import { onHostKeyVerificationNeeded } from "../events";
import type { HostKeyVerificationNeededEvent, ServerDto } from "../types";
import { HostKeyDialog } from "./HostKeyDialog";

interface ServerListProps {
  onConnected: (sessionId: string, serverName: string, serverId: string) => void;
}

/**
 * Spec 0007, Abschnitt 7: Klick auf einen Server löst `connect()` aus.
 * Reagiert auf `host-key-verification-needed` mit `HostKeyDialog` — der
 * Event-Listener läuft, solange irgendein `connect()` dieser Liste
 * unterwegs ist (nicht dauerhaft), da die `session_id` im Event erst durch
 * den laufenden `connect()`-Aufruf entsteht (s. Backend-Kommentar zu
 * `commands::connect`).
 */
export function ServerList({ onConnected }: ServerListProps) {
  const [servers, setServers] = useState<ServerDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [pendingHostKey, setPendingHostKey] = useState<HostKeyVerificationNeededEvent | null>(
    null,
  );

  useEffect(() => {
    listServers()
      .then(setServers)
      .catch((err) => setError(commandErrorMessage(err)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    const unlisten = onHostKeyVerificationNeeded((event) => setPendingHostKey(event));
    return () => {
      unlisten.then((unlistenFn) => unlistenFn());
    };
  }, []);

  const handleConnect = async (server: ServerDto) => {
    setError(null);
    setConnectingId(server.id);
    try {
      const sessionId = await connect(server.id);
      onConnected(sessionId, server.name, server.id);
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setConnectingId(null);
      setPendingHostKey(null);
    }
  };

  const handleHostKeyDecision = async (decision: Parameters<typeof confirmHostKey>[1]) => {
    if (!pendingHostKey) return;
    const sessionId = pendingHostKey.sessionId;
    setPendingHostKey(null);
    try {
      await confirmHostKey(sessionId, decision);
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  if (loading) {
    return <p className="text-sm text-slate-400">Lade Server…</p>;
  }

  if (servers.length === 0 && !error) {
    return (
      <p className="text-sm text-slate-400">
        Noch keine Server angelegt (s. <code>profiles_demo</code>-Beispiel oder
        CLI-Helfer, solange es noch keine Anlege-UI gibt).
      </p>
    );
  }

  return (
    <>
      {error && <p className="mb-2 text-sm text-red-400">{error}</p>}
      <ul className="divide-y divide-slate-700 rounded-md border border-slate-700">
        {servers.map((server) => (
          <li key={server.id}>
            <button
              type="button"
              onClick={() => handleConnect(server)}
              disabled={connectingId !== null}
              className="flex w-full items-center justify-between px-4 py-3 text-left hover:bg-slate-800 disabled:opacity-60"
            >
              <div>
                <p className="font-medium text-slate-100">{server.name}</p>
                <p className="text-sm text-slate-400">
                  {server.username}@{server.host}:{server.port}
                </p>
              </div>
              <div className="flex items-center gap-2">
                {server.tags.map((tag) => (
                  <span key={tag} className="rounded bg-slate-700 px-2 py-0.5 text-xs text-slate-200">
                    {tag}
                  </span>
                ))}
                {connectingId === server.id && (
                  <span className="text-xs text-indigo-300">Verbinde…</span>
                )}
              </div>
            </button>
          </li>
        ))}
      </ul>

      {pendingHostKey && (
        <HostKeyDialog event={pendingHostKey} onDecision={handleHostKeyDecision} />
      )}
    </>
  );
}
