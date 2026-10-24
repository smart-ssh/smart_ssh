import { useCallback, useState, type FocusEvent, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { numberedServerName, type SessionTab } from "../useSessionTabs";
import { NewSessionMenu } from "./NewSessionMenu";
import { SessionTabContent, tabDisplayLabel } from "./SessionTabParts";

const FOCUS_RING =
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 focus-visible:ring-offset-1 focus-visible:ring-offset-slate-900";

interface SessionTabBarProps {
  tabs: SessionTab[];
  activeSessionId: string | null;
  onSwitch: (sessionId: string | null) => void;
  onRequestClose: (sessionId: string) => void;
  /** Issue #657: opens an additional session to the tab's server. */
  onNewSession?: (server: { id: string; name: string }) => void;
}

/**
 * Spec 0017, Abschnitt 3: ein Tab pro offener Session (Servername,
 * bei MCP-Tabs "<Client> @ <Server>" samt MCP-Abzeichen, Spec 0104,
 * Statuspunkt, Schließen-Button), plus eine feste "Übersicht"-Kachel, die zu
 * den Server-/Verwaltungs-Screens zurückführt, ohne die im Hintergrund
 * offenen Sessions zu schließen — nur sichtbar, sobald mindestens ein Tab
 * offen ist (sonst zeigt `MainScreen` ohnehin schon dieselbe Übersicht).
 *
 * **Kein `data-tauri-drag-region`** auf den Tabs/Buttons selbst (Spec 0014,
 * Abschnitt 5 / Spec 0017, Abschnitt 3) — die bleiben klickbar statt das
 * Fenster zu ziehen. Das Attribut vererbt sich aber **nicht** automatisch
 * von `<AppHeader>` auf diesen Container (Tauri prüft pro Element, kein
 * Vererben über Nachfahren) — deshalb trägt der eigene Root-`<div>` hier
 * das Attribut erneut selbst (wie jede Zwischenebene in `AppHeader`), sonst
 * würde dieser volle, deckende Container die gesamte Fläche zum
 * "Loch" machen, sobald mindestens ein Tab existiert — nicht nur die
 * Buttons selbst.
 */
export function SessionTabBar({ tabs, activeSessionId, onSwitch, onRequestClose, onNewSession }: SessionTabBarProps) {
  const { t } = useTranslation();
  // Roving Tab stop (docs/ui-guidelines.md, "Keyboard"): the strip is one Tab
  // stop; arrow keys, Home and End move between the tabs. `focused` follows the
  // keyboard focus, otherwise the active tab is the stop.
  const [focused, setFocused] = useState<string | null | undefined>(undefined);
  const [menu, setMenu] = useState<{ x: number; y: number; tab: SessionTab } | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  if (tabs.length === 0) return null;
  const stopId = focused !== undefined && (focused === null || tabs.some((x) => x.sessionId === focused)) ? focused : activeSessionId;

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const stops = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>("[data-tab-stop]"));
    const current = stops.indexOf(e.target as HTMLButtonElement);
    if (current < 0) return;
    const key = e.key;
    if (key === "Delete" && tabs.length > 0 && current > 0) {
      e.preventDefault();
      onRequestClose(tabs[current - 1]!.sessionId);
      return;
    }
    let next: number;
    if (key === "ArrowRight") next = (current + 1) % stops.length;
    else if (key === "ArrowLeft") next = (current - 1 + stops.length) % stops.length;
    else if (key === "Home") next = 0;
    else if (key === "End") next = stops.length - 1;
    else return;
    e.preventDefault();
    stops[next]!.focus();
  };
  const onBlur = (e: FocusEvent<HTMLDivElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget)) setFocused(undefined);
  };
  const stopProps = (id: string | null) => ({
    "data-tab-stop": "",
    tabIndex: stopId === id ? 0 : -1,
    onFocus: () => setFocused(id),
  });

  return (
    // eslint-disable-next-line jsx-a11y/prefer-tag-over-role, jsx-a11y/interactive-supports-focus -- Werkzeugleiste mit Roving-Tabindex; die Pfeiltasten werden hier abgefangen, der Fokus liegt auf den Buttons darin.
    <div
      data-tauri-drag-region
      role="toolbar"
      aria-orientation="horizontal"
      onKeyDown={onKeyDown}
      onBlur={onBlur}
      className="flex w-full items-center gap-1 overflow-x-auto"
    >
      <button
        type="button"
        {...stopProps(null)}
        aria-current={activeSessionId === null ? "page" : undefined}
        onClick={() => onSwitch(null)}
        title={t("sessionTabs.overview")}
        className={`${FOCUS_RING} font-heading shrink-0 border px-2.5 py-1 text-xs font-semibold tracking-wide ${
          activeSessionId === null
            ? "border-indigo-600/55 bg-indigo-600/16 text-indigo-400"
            : "border-transparent text-slate-400 hover:bg-slate-800"
        }`}
      >
        {t("sessionTabs.overview")}
      </button>

      {tabs.map((tab) => {
        const active = tab.sessionId === activeSessionId;
        // Spec 0104: MCP-Tabs heißen "<Client> @ <Server>", tragen ein
        // MCP-Abzeichen und zeigen eine wartende Bestätigung als
        // beschriftetes Abzeichen statt nur als Punkt — sie werden nie von
        // selbst aktiv, der Hinweis muss also aus dem Hintergrund auffallen.
        const label = tabDisplayLabel(tab, t);
        return (
          <div
            key={tab.sessionId}
            data-tauri-drag-region
            onContextMenu={
              onNewSession && !tab.mcp
                ? (e) => {
                    e.preventDefault();
                    setMenu({ x: e.clientX, y: e.clientY, tab });
                  }
                : undefined
            }
            className={`group flex shrink-0 items-center gap-1.5 border px-2 py-1 text-xs ${
              active
                ? "border-indigo-600/55 bg-indigo-600/16 text-indigo-300"
                : "border-transparent text-slate-400 hover:bg-slate-800"
            }`}
          >
            <button
              type="button"
              {...stopProps(tab.sessionId)}
              aria-current={active ? "page" : undefined}
              onClick={() => onSwitch(tab.sessionId)}
              className={`${FOCUS_RING} font-heading flex items-center gap-1.5 font-semibold tracking-wide`}
              title={
                tab.mcp
                  ? `${label} — ${t("sessionTabs.mcp.tabHint")}`
                  : tab.phase === "connected"
                    ? numberedServerName(tab)
                    : label
              }
            >
              <SessionTabContent
                tab={tab}
                t={t}
                label={label}
                labelClass={tab.mcp ? "max-w-[24ch] truncate" : tab.phase !== "connected" ? "max-w-[28ch] truncate" : "max-w-[12ch] truncate"}
              />
            </button>
            <button
              type="button"
              onClick={() => onRequestClose(tab.sessionId)}
              aria-label={t("sessionTabs.closeTab", { name: label })}
              // Eigener Tab-Stopp nur im aktiven Tab; die übrigen schließt die
              // Entf-Taste auf dem fokussierten Tab (Roving-Tabindex).
              tabIndex={active ? 0 : -1}
              className={`${FOCUS_RING} inline-flex h-5 w-5 shrink-0 items-center justify-center rounded border border-transparent text-slate-400 hover:border-slate-500 hover:bg-slate-600/60 hover:text-slate-200 focus-visible:border-slate-500 focus-visible:bg-slate-600/60 focus-visible:text-slate-200`}
            >
              ✕
            </button>
          </div>
        );
      })}
      {menu && onNewSession && (
        <NewSessionMenu
          x={menu.x}
          y={menu.y}
          serverName={menu.tab.serverName}
          onClose={closeMenu}
          onNewSession={() => {
            setMenu(null);
            onNewSession({ id: menu.tab.serverId, name: menu.tab.serverName });
          }}
        />
      )}
    </div>
  );
}
