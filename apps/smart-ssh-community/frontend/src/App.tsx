import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AiProviderSettings } from "./components/AiProviderSettings";
import { AppHeader } from "./components/AppHeader";
import { FilterRulesView } from "./components/FilterRulesView";
import { ManagementView } from "./components/ManagementView";
import { NoteSuggestionToast } from "./components/NoteSuggestionToast";
import { ServerList } from "./components/ServerList";
import { SessionTabBar } from "./components/SessionTabBar";
import { SessionView } from "./components/SessionView";
import { FeatureLockedDialog } from "./extensions/FeatureLockedDialog";
import { commandErrorMessage, listAiProviders } from "./api";
import { useSessionTabs } from "./useSessionTabs";

type Tab = "connect" | "manage" | "rules";

function App() {
  const [tab, setTab] = useState<Tab>("connect");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [hasActiveProvider, setHasActiveProvider] = useState<boolean | null>(null);
  const {
    tabs: sessionTabs,
    activeSessionId,
    openTab,
    findExistingSessionId,
    switchTo,
    markActionSettled,
    requestCloseTab,
  } = useSessionTabs();

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

  // Spec 0017, Abschnitt 3: `Cmd`/`Ctrl+W` schließt den aktiven Tab,
  // `Cmd`/`Ctrl+Tab` wechselt zum nächsten, `Cmd`/`Ctrl+1..9` springt direkt
  // zum n-ten Tab. `Cmd+Tab` wird auf macOS vom Betriebssystem selbst als
  // App-Wechsler abgefangen und erreicht den Webview nie — auf
  // Windows/Linux (`Ctrl+Tab`, kein OS-reserviertes Kürzel dort) funktioniert
  // der Handler wie vorgesehen; auf macOS bleiben `Cmd+W`/`Cmd+1..9` als
  // Alternative.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!e.metaKey && !e.ctrlKey) return;
      if (e.key === "w" || e.key === "W") {
        if (!activeSessionId) return;
        e.preventDefault();
        requestCloseTab(activeSessionId);
        return;
      }
      if (e.key === "Tab") {
        if (sessionTabs.length === 0) return;
        e.preventDefault();
        const currentIndex = sessionTabs.findIndex((t) => t.sessionId === activeSessionId);
        const nextIndex = (currentIndex + 1) % sessionTabs.length;
        switchTo(sessionTabs[nextIndex].sessionId);
        return;
      }
      if (/^[1-9]$/.test(e.key)) {
        const target = sessionTabs[Number(e.key) - 1];
        if (!target) return;
        e.preventDefault();
        switchTo(target.sessionId);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [sessionTabs, activeSessionId, requestCloseTab, switchTo]);

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
      <AppHeader>
        <SessionTabBar
          tabs={sessionTabs}
          activeSessionId={activeSessionId}
          onSwitch={switchTo}
          onRequestClose={requestCloseTab}
        />
      </AppHeader>
      <NoteSuggestionToast />

      {/* Spec 0017, Abschnitt 4: jede offene Session bleibt gemountet
       * (eigener Chat-Verlauf, eigene xterm.js-Instanz samt Scrollback,
       * eigener Bestätigungsdialog-/Prompt-Historie-Zustand lebt bereits
       * lokal in `ChatPanel`/`TerminalView`) — nur per CSS ausgeblendet,
       * statt beim Tab-Wechsel neu gemountet zu werden. Ein Unmount würde
       * die xterm-Instanz samt Scrollback und den gesamten Chat-Verlauf
       * verwerfen, sobald der Nutzer zu einem anderen Tab wechselt. */}
      {sessionTabs.map((sessionTab) => (
        <div
          key={sessionTab.sessionId}
          className={
            sessionTab.sessionId === activeSessionId ? "flex flex-1 min-h-0 flex-col" : "hidden"
          }
        >
          <SessionView
            sessionId={sessionTab.sessionId}
            serverName={sessionTab.serverName}
            serverId={sessionTab.serverId}
            onRequestClose={() => requestCloseTab(sessionTab.sessionId)}
            onActionSettled={markActionSettled}
            isActiveTab={sessionTab.sessionId === activeSessionId}
          />
        </div>
      ))}

      <div className={activeSessionId === null ? "flex flex-1 min-h-0 flex-col" : "hidden"}>
        <MainScreen
          tab={tab}
          setTab={setTab}
          settingsOpen={settingsOpen}
          setSettingsOpen={setSettingsOpen}
          hasActiveProvider={hasActiveProvider}
          refreshProviderStatus={refreshProviderStatus}
          findExistingSessionId={findExistingSessionId}
          onSwitchToExistingTab={switchTo}
          onConnected={(sessionId, serverName, serverId) => openTab(sessionId, serverId, serverName)}
        />
      </div>
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
  findExistingSessionId: (serverId: string) => string | undefined;
  onSwitchToExistingTab: (sessionId: string) => void;
}

function MainScreen({
  tab,
  setTab,
  settingsOpen,
  setSettingsOpen,
  hasActiveProvider,
  refreshProviderStatus,
  onConnected,
  findExistingSessionId,
  onSwitchToExistingTab,
}: MainScreenProps) {
  const { t } = useTranslation();
  // Spec 0033, Abschnitt 4: hier statt in `ServerList` selbst gehalten,
  // damit der Auf-/Zuklapp-Zustand einen Tab-Wechsel weg von "Verbinden"
  // übersteht (`ServerList` wird beim Wechsel zu "Verwalten"/"Filter-Regeln"
  // unten unmounted) — `MainScreen` bleibt für die gesamte Sitzung gemountet.
  const [collapsedGroupIds, setCollapsedGroupIds] = useState<Set<string>>(new Set());
  const toggleGroup = (groupId: string) => {
    setCollapsedGroupIds((prev) => {
      const next = new Set(prev);
      if (next.has(groupId)) {
        next.delete(groupId);
      } else {
        next.add(groupId);
      }
      return next;
    });
  };
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
              {t("nav.connect")}
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
              {t("nav.manage")}
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
              {t("nav.rules")}
            </button>
          </nav>
        </div>
        <button
          type="button"
          onClick={() => setSettingsOpen(true)}
          className="font-heading border border-slate-700 bg-slate-800 px-3 py-1.5 text-sm font-semibold tracking-wide hover:bg-slate-700"
        >
          {t("settings.title")}
        </button>
      </header>

      {tab === "connect" ? (
        <main className="mx-auto w-full max-w-3xl flex-1 space-y-4 overflow-y-auto px-6 py-8">
          {hasActiveProvider === false && (
            <p className="rounded border border-amber-800 bg-amber-950 px-4 py-3 text-sm text-amber-200">
              {t("mainScreen.noActiveProvider")}{" "}
              <button
                type="button"
                onClick={() => setSettingsOpen(true)}
                className="underline hover:no-underline"
              >
                {t("mainScreen.setupProviderNow")}
              </button>
              .
            </p>
          )}

          <section>
            <h2 className="mb-2 text-sm font-semibold uppercase tracking-wide text-slate-400">
              {t("mainScreen.serversHeading")}
            </h2>
            <ServerList
              onConnected={onConnected}
              findExistingSessionId={findExistingSessionId}
              onSwitchToExistingTab={onSwitchToExistingTab}
              collapsedGroupIds={collapsedGroupIds}
              onToggleGroup={toggleGroup}
            />
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

      <FeatureLockedDialog />
    </div>
  );
}

export default App;
