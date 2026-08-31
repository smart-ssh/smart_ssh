import { useEffect, useState } from "react";
import { AiProviderSettings } from "./components/AiProviderSettings";
import { ServerList } from "./components/ServerList";
import { SessionView } from "./components/SessionView";
import { commandErrorMessage, listAiProviders } from "./api";

interface ActiveSession {
  sessionId: string;
  serverName: string;
}

function App() {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [hasActiveProvider, setHasActiveProvider] = useState<boolean | null>(null);
  const [activeSession, setActiveSession] = useState<ActiveSession | null>(null);

  const refreshProviderStatus = () => {
    listAiProviders()
      .then((providers) => setHasActiveProvider(providers.some((p) => p.isActive)))
      .catch((err) => {
        // Nur fürs Hinweis-Banner relevant — ein Fehler hier soll nicht die
        // ganze Seite blockieren, `AiProviderSettings` zeigt eigene Fehler
        // ohnehin selbst an.
        console.error(commandErrorMessage(err));
      });
  };

  useEffect(refreshProviderStatus, []);

  if (activeSession) {
    return (
      <SessionView
        sessionId={activeSession.sessionId}
        serverName={activeSession.serverName}
        onDisconnected={() => setActiveSession(null)}
      />
    );
  }

  return (
    <div className="min-h-screen bg-slate-900 text-slate-100">
      <header className="flex items-center justify-between border-b border-slate-800 px-6 py-4">
        <h1 className="text-xl font-semibold">ssh-manager</h1>
        <button
          type="button"
          onClick={() => setSettingsOpen(true)}
          className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700"
        >
          Einstellungen
        </button>
      </header>

      <main className="mx-auto max-w-3xl space-y-4 px-6 py-8">
        {hasActiveProvider === false && (
          <p className="rounded border border-amber-800 bg-amber-950 px-4 py-3 text-sm text-amber-200">
            Noch kein aktiver AI-Provider konfiguriert.{" "}
            <button
              type="button"
              onClick={() => setSettingsOpen(true)}
              className="underline hover:no-underline"
            >
              Jetzt in den Einstellungen einrichten
            </button>
            .
          </p>
        )}

        <section>
          <h2 className="mb-2 text-sm font-semibold uppercase tracking-wide text-slate-400">
            Server
          </h2>
          <ServerList
            onConnected={(sessionId, serverName) => setActiveSession({ sessionId, serverName })}
          />
        </section>
      </main>

      {settingsOpen && (
        <AiProviderSettings
          onClose={() => setSettingsOpen(false)}
          onProvidersChanged={refreshProviderStatus}
        />
      )}
    </div>
  );
}

export default App;
