import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AppHeader } from "./components/AppHeader";
import { FilterRulesView } from "./components/FilterRulesView";
import { ManagementView } from "./components/ManagementView";
import type { Selection } from "./components/Sidebar";
import { NoteShrinkSuggestionToast } from "./components/NoteShrinkSuggestionToast";
import { NoteSuggestionToast } from "./components/NoteSuggestionToast";
import { Button } from "./components/ui";
import { ToastHost } from "./components/ToastHost";
import { HostKeyPromptHost } from "./components/HostKeyPromptHost";
import { ServerList } from "./components/ServerList";
import { NarrowHeaderNav } from "./components/NarrowHeaderNav";
import { SessionTabBar } from "./components/SessionTabBar";
import { ConnectingTabView } from "./components/ConnectingTabView";
import { SessionView } from "./components/SessionView";
import { SettingsScreen } from "./components/SettingsScreen";
import { FeatureLockedDialog } from "./extensions/FeatureLockedDialog";
import { commandErrorMessage, listAiProviders } from "./api";
import {
  subscribeRequestResumeChatSession,
  subscribeRequestOpenFilterRule,
  subscribeRequestServerNoteEdit,
  type ResumeChatSessionRequest,
} from "./navigationBus";
import { useIsNarrow } from "./hooks/useIsNarrow";
import { MAIN_TAB_ENTRIES, type MainTab } from "./navEntries";
import { useSessionTabs } from "./useSessionTabs";

const FOCUS_RING =
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 focus-visible:ring-offset-1 focus-visible:ring-offset-slate-900";

type Tab = MainTab;

function App() {
  const [tab, setTab] = useState<Tab>("connect");
  const [settingsOpen, setSettingsOpen] = useState(false);
  // WebKit on macOS does not focus a clicked button, so the opener is taken
  // from the click and handed to the dialog as the focus-return target.
  const [settingsOpener, setSettingsOpener] = useState<HTMLElement | null>(null);
  const openSettingsFrom = (opener: HTMLElement) => {
    setSettingsOpener(opener);
    setSettingsOpen(true);
  };
  const narrow = useIsNarrow();
  const [hasActiveProvider, setHasActiveProvider] = useState<boolean | null>(null);
  // Spec 0057, §4.2 (Etappe 4): "Mache ich selbst" springt zur
  // Server-Bearbeitung — s. `navigationBus.ts`-Doc-Kommentar. Lebt hier
  // (statt in `MainScreen`, das `tab` selbst hält), weil auch
  // `activeSessionId` (unten, aus `useSessionTabs`) hier auf `null` gesetzt
  // werden muss, um überhaupt zu `MainScreen`/`ManagementView` zu wechseln.
  const [pendingNoteEditSelection, setPendingNoteEditSelection] = useState<Selection | null>(
    null,
  );
  const {
    tabs: sessionTabs,
    activeSessionId,
    startConnect,
    retryConnect,
    findExistingSessionId,
    switchTo,
    markActionSettled,
    requestCloseTab,
  } = useSessionTabs();

  // Issue #611: an extension asks to resume a saved chat session. An open
  // tab of the server wins (Spec 0017 §3); otherwise the overview takes the
  // request and runs the picker's resume path (first-run gate, connecting tab).
  const [resumeRequest, setResumeRequest] = useState<ResumeChatSessionRequest | null>(null);
  useEffect(
    () =>
      subscribeRequestResumeChatSession((request) => {
        const existing = findExistingSessionId(request.serverId);
        if (existing) {
          switchTo(existing);
          return;
        }
        switchTo(null);
        setTab("connect");
        setResumeRequest(request);
      }),
    [findExistingSessionId, switchTo],
  );

  useEffect(
    () =>
      subscribeRequestServerNoteEdit((serverId) => {
        switchTo(null);
        setTab("manage");
        setPendingNoteEditSelection({ kind: "server", id: serverId });
      }),
    [switchTo],
  );

  // Issue #696: a chat card opens one rule in the filter rules tab.
  const [pendingRuleSelection, setPendingRuleSelection] = useState<string | null>(null);
  useEffect(
    () =>
      subscribeRequestOpenFilterRule((ruleId) => {
        switchTo(null);
        setTab("rules");
        setPendingRuleSelection(ruleId);
      }),
    [switchTo],
  );

  /** Spec 0069, Teil C1 (BL-0082): "Ersten Server anlegen" im
   * Einstiegs-Block von `ServerList` — derselbe `pendingNoteEditSelection`-
   * Mechanismus wie oben (Spec 0057, §4.2), hier nur mit `newServer` statt
   * `server`. Kein `switchTo(null)` nötig: `ServerList` ist ohnehin nur
   * sichtbar, wenn kein Session-Tab aktiv ist (s. `activeSessionId === null`-
   * Zweig unten).
   *
   * Issue #49: dasselbe für „+" an einer Gruppenzeile der Server-Liste,
   * nur mit dieser Gruppe als Vorgabe. */
  const handleCreateServerInGroup = (groupId: string | null) => {
    setTab("manage");
    setPendingNoteEditSelection({ kind: "newServer", groupId });
  };
  const handleCreateFirstServer = () => handleCreateServerInGroup(null);

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

  const connectingServerIds = new Set(
    sessionTabs.filter((t) => t.phase === "connecting").map((t) => t.serverId),
  );

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
        {narrow ? (
          <NarrowHeaderNav
            tabs={sessionTabs}
            activeSessionId={activeSessionId}
            tab={tab}
            onSelectMainTab={(next) => {
              switchTo(null);
              setTab(next);
            }}
            onSwitch={switchTo}
            onRequestClose={requestCloseTab}
            onOpenSettings={openSettingsFrom}
          />
        ) : (
          <SessionTabBar
            tabs={sessionTabs}
            activeSessionId={activeSessionId}
            onSwitch={switchTo}
            onRequestClose={requestCloseTab}
          />
        )}
      </AppHeader>
      <NoteSuggestionToast />
      <NoteShrinkSuggestionToast />
      <ToastHost />
      {/* Issue #12 / ADR 0104: Host-Key-Abfragen kommen auch für vom
       * Backend ausgelöste Verbindungen (z. B. MCP) — daher hier, stets
       * gemountet, statt in `ServerList`, das nur im "Verbinden"-Tab lebt. */}
      <HostKeyPromptHost />

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
          {sessionTab.phase !== "connected" ? (
            <ConnectingTabView
              tab={sessionTab}
              isActiveTab={sessionTab.sessionId === activeSessionId}
              onRetry={() => retryConnect(sessionTab.sessionId)}
              onClose={() => requestCloseTab(sessionTab.sessionId)}
            />
          ) : (
          <SessionView
            sessionId={sessionTab.sessionId}
            serverName={sessionTab.serverName}
            serverId={sessionTab.serverId}
            mcp={sessionTab.mcp}
            onRequestClose={() => requestCloseTab(sessionTab.sessionId)}
            onActionSettled={markActionSettled}
            isActiveTab={sessionTab.sessionId === activeSessionId}
          />
          )}
        </div>
      ))}

      <div className={activeSessionId === null ? "flex flex-1 min-h-0 flex-col" : "hidden"}>
        <MainScreen
          tab={tab}
          setTab={setTab}
          narrow={narrow}
          settingsOpen={settingsOpen}
          settingsOpener={settingsOpener}
          setSettingsOpen={setSettingsOpen}
          openSettingsFrom={openSettingsFrom}
          hasActiveProvider={hasActiveProvider}
          refreshProviderStatus={refreshProviderStatus}
          findExistingSessionId={findExistingSessionId}
          onSwitchToExistingTab={switchTo}
          onStartConnect={startConnect}
          connectingServerIds={connectingServerIds}
          resumeRequest={resumeRequest}
          onResumeRequestConsumed={() => setResumeRequest(null)}
          pendingNoteEditSelection={pendingNoteEditSelection}
          onNoteEditSelectionConsumed={() => setPendingNoteEditSelection(null)}
          pendingRuleSelection={pendingRuleSelection}
          onRuleSelectionConsumed={() => setPendingRuleSelection(null)}
          onCreateFirstServer={handleCreateFirstServer}
          onCreateServerInGroup={handleCreateServerInGroup}
        />
      </div>
    </div>
  );
}

interface MainScreenProps {
  tab: Tab;
  setTab: (tab: Tab) => void;
  narrow: boolean;
  settingsOpen: boolean;
  settingsOpener: HTMLElement | null;
  setSettingsOpen: (open: boolean) => void;
  openSettingsFrom: (opener: HTMLElement) => void;
  hasActiveProvider: boolean | null;
  /** Spec 0057, §4.2 (Etappe 4) — s. `App`s Doc-Kommentar zum
   * `navigationBus`. */
  pendingNoteEditSelection: Selection | null;
  onNoteEditSelectionConsumed: () => void;
  /** Issue #696. */
  pendingRuleSelection: string | null;
  onRuleSelectionConsumed: () => void;
  /** Issue #611. */
  resumeRequest: ResumeChatSessionRequest | null;
  onResumeRequestConsumed: () => void;
  refreshProviderStatus: () => void;
  /** Issue #548. */
  onStartConnect: (server: { id: string; name: string }, resumeChatSessionId?: string) => void;
  connectingServerIds: ReadonlySet<string>;
  findExistingSessionId: (serverId: string) => string | undefined;
  onSwitchToExistingTab: (sessionId: string) => void;
  /** Spec 0069, Teil C1 (BL-0082). */
  onCreateFirstServer: () => void;
  /** Issue #49. */
  onCreateServerInGroup: (groupId: string) => void;
}

function MainScreen({
  tab,
  setTab,
  narrow,
  settingsOpen,
  settingsOpener,
  setSettingsOpen: setSettingsOpenRaw,
  openSettingsFrom: openSettingsRaw,
  hasActiveProvider,
  refreshProviderStatus,
  onStartConnect,
  connectingServerIds,
  findExistingSessionId,
  onSwitchToExistingTab,
  pendingNoteEditSelection,
  onNoteEditSelectionConsumed,
  pendingRuleSelection,
  onRuleSelectionConsumed,
  resumeRequest,
  onResumeRequestConsumed,
  onCreateFirstServer,
  onCreateServerInGroup,
}: MainScreenProps) {
  const { t } = useTranslation();
  const openSettingsFrom = (event: { currentTarget: HTMLElement }) => openSettingsRaw(event.currentTarget);
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
      {!narrow && (
      <header className="flex items-center justify-between border-b border-slate-800 px-6 py-4">
        <div className="flex items-center gap-6">
          <h1 className="font-heading text-xl font-semibold tracking-wide">Smart SSH</h1>
          <nav className="flex gap-1">
            {MAIN_TAB_ENTRIES.map((entry) => (
              <button
                key={entry.id}
                type="button"
                onClick={() => setTab(entry.id)}
                aria-current={tab === entry.id ? "page" : undefined}
                className={`${FOCUS_RING} font-heading border px-3 py-1.5 text-sm font-semibold tracking-wide ${
                  tab === entry.id
                    ? "border-indigo-600/55 bg-indigo-600/16 text-indigo-400"
                    : "border-transparent text-slate-400 hover:bg-slate-800"
                }`}
              >
                {t(entry.labelKey)}
              </button>
            ))}
          </nav>
        </div>
        <Button onClick={openSettingsFrom}>{t("settings.title")}</Button>
      </header>
      )}

      {tab === "connect" ? (
        <main className="mx-auto w-full max-w-3xl flex-1 space-y-4 overflow-y-auto px-6 py-8 narrow:px-3 narrow:py-4">
          {hasActiveProvider === false && (
            <p className="rounded border border-amber-800 bg-amber-950 px-4 py-3 text-sm text-amber-200">
              {t("mainScreen.noActiveProvider")}{" "}
              <button
                type="button"
                onClick={openSettingsFrom}
                className="rounded underline hover:no-underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-300"
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
              onStartConnect={onStartConnect}
              connectingServerIds={connectingServerIds}
              findExistingSessionId={findExistingSessionId}
              onSwitchToExistingTab={onSwitchToExistingTab}
              collapsedGroupIds={collapsedGroupIds}
              onToggleGroup={toggleGroup}
              resumeRequest={resumeRequest}
              onResumeRequestConsumed={onResumeRequestConsumed}
              onCreateFirstServer={onCreateFirstServer}
              onCreateServerInGroup={onCreateServerInGroup}
            />
          </section>
        </main>
      ) : tab === "manage" ? (
        <ManagementView
          initialSelection={pendingNoteEditSelection}
          onInitialSelectionConsumed={onNoteEditSelectionConsumed}
        />
      ) : (
        <FilterRulesView
          initialRuleId={pendingRuleSelection}
          onInitialRuleConsumed={onRuleSelectionConsumed}
        />
      )}

      {settingsOpen && (
        <SettingsScreen
          returnFocusTo={settingsOpener}
          onClose={() => setSettingsOpenRaw(false)}
          onProvidersChanged={refreshProviderStatus}
        />
      )}

      <FeatureLockedDialog />
    </div>
  );
}

export default App;
