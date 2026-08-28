import { useEffect, useState } from "react";
import { commandErrorMessage, listServers } from "../api";
import type { ServerDto } from "../types";

/**
 * Reine Anzeigeliste (Spec 0007, Abschnitt 7: "keine Anlege-/
 * Bearbeiten-UI für Server/Gruppen in diesem Schritt"). Kein Klick-
 * Handler — der kommt erst mit dem Verbindungs-Teil.
 */
export function ServerList() {
  const [servers, setServers] = useState<ServerDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    listServers()
      .then(setServers)
      .catch((err) => setError(commandErrorMessage(err)))
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return <p className="text-sm text-slate-400">Lade Server…</p>;
  }

  if (error) {
    return <p className="text-sm text-red-400">Fehler beim Laden der Server: {error}</p>;
  }

  if (servers.length === 0) {
    return (
      <p className="text-sm text-slate-400">
        Noch keine Server angelegt (s. <code>profiles_demo</code>-Beispiel oder
        CLI-Helfer, solange es noch keine Anlege-UI gibt).
      </p>
    );
  }

  return (
    <ul className="divide-y divide-slate-700 rounded-md border border-slate-700">
      {servers.map((server) => (
        <li key={server.id} className="flex items-center justify-between px-4 py-3">
          <div>
            <p className="font-medium text-slate-100">{server.name}</p>
            <p className="text-sm text-slate-400">
              {server.username}@{server.host}:{server.port}
            </p>
          </div>
          {server.tags.length > 0 && (
            <div className="flex gap-1">
              {server.tags.map((tag) => (
                <span
                  key={tag}
                  className="rounded bg-slate-700 px-2 py-0.5 text-xs text-slate-200"
                >
                  {tag}
                </span>
              ))}
            </div>
          )}
        </li>
      ))}
    </ul>
  );
}
