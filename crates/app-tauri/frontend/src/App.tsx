import { useEffect, useState } from "react";
import { AiProviderSettings } from "./components/AiProviderSettings";
import { FilterRulesView } from "./components/FilterRulesView";
import { ManagementView } from "./components/ManagementView";
import { NoteSuggestionToast } from "./components/NoteSuggestionToast";
import { ServerList } from "./components/ServerList";
import { SessionView } from "./components/SessionView";
import { commandErrorMessage, listAiProviders } from "./api";

interface ActiveSession {
  sessionId: string;
  serverName: string;
}

type Tab = "connect" | "manage" | "rules";

function App() {
  const [tab, setTab] = useState<Tab>("connect");
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

  return (
    // Spec 0010, Abschnitt 2, Punkt 6: die Benachrichtigung muss auch dann
    // noch ankommen, wenn der Nutzer den Session-Screen bereits verlassen
    // hat — als einziger, stets gemounteter Listener auf dieser äußersten
    // Ebene statt dupliziert an einen der beiden Zweige unten gebunden
    // (das würde bei jedem Wechsel zwischen Session-/Tab-Ansicht einen
    // unnötigen Remount auslösen).
    <>
      <NoteSuggestionToast />
      {activeSession ? (
        <SessionView
          sessionId={activeSession.sessionId}
          serverName={activeSession.serverName}
          onDisconnected={() => setActiveSession(null)}
        />
      ) : (
        <MainScreen
          tab={tab}
          setTab={setTab}
          settingsOpen={settingsOpen}
          setSettingsOpen={setSettingsOpen}
          hasActiveProvider={hasActiveProvider}
          refreshProviderStatus={refreshProviderStatus}
          onConnected={(sessionId, serverName) => setActiveSession({ sessionId, serverName })}
        />
      )}
    </>
  );
}

interface MainScreenProps {
  tab: Tab;
  setTab: (tab: Tab) => void;
  settingsOpen: boolean;
  setSettingsOpen: (open: boolean) => void;
  hasActiveProvider: boolean | null;
  refreshProviderStatus: () => void;
  onConnected: (sessionId: string, serverName: string) => void;
}

function MainScreen({
  tab,
  setTab,
  settingsOpen,
  setSettingsOpen,
  hasActiveProvider,
  refreshProviderStatus,
  onConnected,
}: MainScreenProps) {
  return (
    <div className="flex h-screen flex-col bg-slate-900 text-slate-100">
      <header className="flex items-center justify-between border-b border-slate-800 px-6 py-4">
        <div className="flex items-center gap-6">
          <h1 className="text-xl font-semibold">Smart SSH</h1>
          <nav className="flex gap-1">
            <button
              type="button"
              onClick={() => setTab("connect")}
              className={`rounded px-3 py-1.5 text-sm ${
                tab === "connect" ? "bg-slate-800 text-white" : "text-slate-400 hover:bg-slate-800"
              }`}
            >
              Verbinden
            </button>
            <button
              type="button"
              onClick={() => setTab("manage")}
              className={`rounded px-3 py-1.5 text-sm ${
                tab === "manage" ? "bg-slate-800 text-white" : "text-slate-400 hover:bg-slate-800"
              }`}
            >
              Verwalten
            </button>
            <button
              type="button"
              onClick={() => setTab("rules")}
              className={`rounded px-3 py-1.5 text-sm ${
                tab === "rules" ? "bg-slate-800 text-white" : "text-slate-400 hover:bg-slate-800"
              }`}
            >
              Filter-Regeln
            </button>
          </nav>
        </div>
        <button
          type="button"
          onClick={() => setSettingsOpen(true)}
          className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700"
        >
          Einstellungen
        </button>
      </header>

      {tab === "connect" ? (
        <main className="mx-auto w-full max-w-3xl flex-1 space-y-4 overflow-y-auto px-6 py-8">
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
            <ServerList onConnected={onConnected} />
          </section>
        </main>
      ) : tab === "manage" ? (
        <ManagementView />
      ) : (
        <FilterRulesView />
      )}

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
