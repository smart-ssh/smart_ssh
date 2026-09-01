import { useEffect, useState } from "react";
import { AiProviderSettings } from "./components/AiProviderSettings";
import { AppHeader } from "./components/AppHeader";
import { FilterRulesView } from "./components/FilterRulesView";
import { ManagementView } from "./components/ManagementView";
import { NoteSuggestionToast } from "./components/NoteSuggestionToast";
import { ServerList } from "./components/ServerList";
import { SessionView } from "./components/SessionView";
import { commandErrorMessage, listAiProviders } from "./api";

interface ActiveSession {
  sessionId: string;
  serverName: string;
  serverId: string;
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
    // Bewusst KEIN `select-none` auf dieser Ebene (s. Spec 0016, Abschnitt
    // 3, Untersuchung Textmarkierung): `user-select: none` vererbt sich auf
    // alle Kindelemente ohne expliziten Gegen-`select-text` — ein
    // `select-none` hier hätte Text im gesamten Chat-/Terminal-/
    // Notizbereich unmarkierbar gemacht. `AppHeader` trägt bereits sein
    // eigenes, korrekt auf die Titelleisten-Drag-Region begrenztes
    // `select-none` (Spec 0014).
    <div className="flex h-screen flex-col bg-slate-900 text-slate-100 overflow-hidden">
      <AppHeader />
      <NoteSuggestionToast />
      {activeSession ? (
        <SessionView
          sessionId={activeSession.sessionId}
          serverName={activeSession.serverName}
          serverId={activeSession.serverId}
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
          onConnected={(sessionId, serverName, serverId) =>
            setActiveSession({ sessionId, serverName, serverId })
          }
        />
      )}
    </div>
  );
}

interface MainScreenProps {
  tab: Tab;
  setTab: (tab: Tab) => void;
  settingsOpen: boolean;
  setSettingsOpen: (open: boolean) => void;
  hasActiveProvider: boolean | null;
  refreshProviderStatus: () => void;
  onConnected: (sessionId: string, serverName: string, serverId: string) => void;
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
    <div className="flex flex-1 min-h-0 flex-col bg-slate-900 text-slate-100">
      <header className="flex items-center justify-between border-b border-slate-800 px-6 py-4">
        <div className="flex items-center gap-6">
          <h1 className="font-heading text-xl font-semibold tracking-wide">Smart SSH</h1>
          <nav className="flex gap-1">
            <button
              type="button"
              onClick={() => setTab("connect")}
              className={`font-heading border px-3 py-1.5 text-sm font-semibold tracking-wide ${
                tab === "connect"
                  ? "border-indigo-600/55 bg-indigo-600/16 text-indigo-400"
                  : "border-transparent text-slate-400 hover:bg-slate-800"
              }`}
            >
              Verbinden
            </button>
            <button
              type="button"
              onClick={() => setTab("manage")}
              className={`font-heading border px-3 py-1.5 text-sm font-semibold tracking-wide ${
                tab === "manage"
                  ? "border-indigo-600/55 bg-indigo-600/16 text-indigo-400"
                  : "border-transparent text-slate-400 hover:bg-slate-800"
              }`}
            >
              Verwalten
            </button>
            <button
              type="button"
              onClick={() => setTab("rules")}
              className={`font-heading border px-3 py-1.5 text-sm font-semibold tracking-wide ${
                tab === "rules"
                  ? "border-indigo-600/55 bg-indigo-600/16 text-indigo-400"
                  : "border-transparent text-slate-400 hover:bg-slate-800"
              }`}
            >
              Filter-Regeln
            </button>
          </nav>
        </div>
        <button
          type="button"
          onClick={() => setSettingsOpen(true)}
          className="font-heading border border-slate-700 bg-slate-800 px-3 py-1.5 text-sm font-semibold tracking-wide hover:bg-slate-700"
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
