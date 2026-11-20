import { useCallback, useState, type FocusEvent, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { numberedServerName, type SessionTab } from "../hooks/useSessionTabs";
import { NewSessionMenu } from "./NewSessionMenu";
import { SessionTabContent, tabDisplayLabel } from "./SessionTabParts";
import { Button, Icon } from "./ui";


interface SessionTabBarProps {
  tabs: SessionTab[];
  activeSessionId: string | null;
  onSwitch: (sessionId: string) => void;
  /** The "+" button at the end of the strip: opens the Connect view. */
  onOpenConnect: () => void;
  onRequestClose: (sessionId: string) => void;
  /** Issue #657: opens an additional session to the tab's server. */
  onNewSession?: (server: { id: string; name: string }) => void;
}

/**
 * Spec 0017, Abschnitt 3: ein Tab pro offener Session (Servername,
 * bei MCP-Tabs "<Client> @ <Server>" samt MCP-Abzeichen, Spec 0104,
 * Statuspunkt, Schließen-Button), plus ein "+"-Button am Ende, der die
 * Verbinden-Ansicht öffnet. Die frühere "Übersicht"-Kachel gibt es nicht
 * mehr: "Verbinden" in der Titelleiste (`AppHeader`) führt zur Übersicht.
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
export function SessionTabBar({ tabs, activeSessionId, onSwitch, onRequestClose, onOpenConnect, onNewSession }: SessionTabBarProps) {
  const { t } = useTranslation();
  // Roving Tab stop (docs/ui-guidelines.md, "Keyboard"): the strip is one Tab
  // stop; arrow keys, Home and End move between the tabs. `focused` follows the
  // keyboard focus, otherwise the active tab is the stop.
  const [focused, setFocused] = useState<string | null | undefined>(undefined);
  const [menu, setMenu] = useState<{ x: number; y: number; tab: SessionTab } | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  if (tabs.length === 0) return null;
  const stopId =
    focused !== undefined && (focused === "+" || tabs.some((x) => x.sessionId === focused))
      ? focused
      : tabs.some((x) => x.sessionId === activeSessionId)
        ? activeSessionId
        : tabs[0]!.sessionId;

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const stops = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>("[data-tab-stop]"));
    const current = stops.indexOf(e.target as HTMLButtonElement);
    if (current < 0) return;
    const key = e.key;
    if (key === "Delete" && current < tabs.length) {
      e.preventDefault();
      onRequestClose(tabs[current]!.sessionId);
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
  const stopProps = (id: string) => ({
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
      className="flex h-full min-w-0 items-stretch overflow-x-auto overflow-y-hidden [scrollbar-width:none]"
    >
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
            className={`group flex min-w-[7rem] shrink items-center gap-2 border-l border-t-2 border-l-border px-3 text-[12.5px] ${
              active
                ? "border-t-accent bg-surface text-fg-strong"
                : "border-t-transparent text-fg-subtle hover:bg-surface-raised hover:text-fg"
            }`}
          >
            <Button variant="bare"
              type="button"
              {...stopProps(tab.sessionId)}
              aria-current={active ? "page" : undefined}
              onClick={() => onSwitch(tab.sessionId)}
              className="focus-ring flex h-full min-w-0 items-center gap-2 font-mono"
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
            </Button>
            <Button variant="bare"
              type="button"
              onClick={() => onRequestClose(tab.sessionId)}
              aria-label={t("sessionTabs.closeTab", { name: label })}
              // Eigener Tab-Stopp nur im aktiven Tab; die übrigen schließt die
              // Entf-Taste auf dem fokussierten Tab (Roving-Tabindex).
              tabIndex={active ? 0 : -1}
              className={`focus-ring inline-flex h-5 w-5 shrink-0 items-center justify-center border border-transparent text-fg-subtle hover:border-fg-faint hover:bg-border-strong/60 hover:text-fg-soft focus-visible:border-fg-faint focus-visible:bg-border-strong/60 focus-visible:text-fg-soft`}
            >
              <Icon name="x" size={12} />
            </Button>
          </div>
        );
      })}
      <Button
        variant="bare"
        type="button"
        {...stopProps("+")}
        onClick={onOpenConnect}
        aria-label={t("sessionTabs.newConnection")}
        title={t("sessionTabs.newConnection")}
        className="focus-ring inline-flex h-full w-10 shrink-0 items-center justify-center border-l border-l-border text-fg-subtle hover:bg-surface-raised hover:text-fg"
      >
        <Icon name="plus" size={14} />
      </Button>
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
