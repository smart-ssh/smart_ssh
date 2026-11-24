import { useCallback, useEffect, useRef, useState, type FocusEvent, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { numberedServerName, type SessionTab } from "../hooks/useSessionTabs";
import { NewSessionMenu } from "./NewSessionMenu";
import { SessionTabContent, tabDisplayLabel } from "./SessionTabParts";
import { Button, Icon } from "./ui";

/** Pixels per line for `WheelEvent.DOM_DELTA_LINE` (a typical browser line height). */
const WHEEL_LINE_PX = 16;

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
  // Overflow of the strip (Spec 0017, section 3): which edges hide tabs. Drives
  // an edge fade and the vertical-wheel-to-horizontal remapping; both are off
  // while everything fits.
  const stripRef = useRef<HTMLDivElement>(null);
  const [overflow, setOverflow] = useState({ start: false, end: false });
  const hasTabs = tabs.length > 0;
  useEffect(() => {
    const el = stripRef.current;
    if (!el) return;
    const update = () => {
      const start = el.scrollLeft > 1;
      const end = el.scrollLeft + el.clientWidth < el.scrollWidth - 1;
      setOverflow((o) => (o.start === start && o.end === end ? o : { start, end }));
    };
    // React's onWheel is passive, so the remapping needs a native listener.
    const onWheel = (e: WheelEvent) => {
      if (el.scrollWidth <= el.clientWidth || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
      e.preventDefault();
      // deltaMode: 0 pixels, 1 lines, 2 pages (a page is one strip width).
      const unit = e.deltaMode === 1 ? WHEEL_LINE_PX : e.deltaMode === 2 ? el.clientWidth : 1;
      el.scrollLeft += e.deltaY * unit;
    };
    update();
    el.addEventListener("scroll", update, { passive: true });
    el.addEventListener("wheel", onWheel, { passive: false });
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(update) : null;
    ro?.observe(el);
    for (const child of Array.from(el.children)) ro?.observe(child);
    return () => {
      el.removeEventListener("scroll", update);
      el.removeEventListener("wheel", onWheel);
      ro?.disconnect();
    };
  }, [hasTabs, tabs.length]);
  const fade = `${overflow.start ? "transparent, currentColor 1.5rem" : "currentColor, currentColor"}, ${overflow.end ? "currentColor calc(100% - 1.5rem), transparent" : "currentColor, currentColor"}`;
  const mask = overflow.start || overflow.end ? `linear-gradient(to right, ${fade})` : undefined;
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
    <>
    {/* The menu is a sibling, not a child, of the masked strip so the mask cannot clip it. */}
    {/* eslint-disable-next-line jsx-a11y/prefer-tag-over-role, jsx-a11y/interactive-supports-focus -- Werkzeugleiste mit Roving-Tabindex; die Pfeiltasten werden hier abgefangen, der Fokus liegt auf den Buttons darin. */}
    <div
      data-tauri-drag-region
      role="toolbar"
      aria-orientation="horizontal"
      onKeyDown={onKeyDown}
      onBlur={onBlur}
      ref={stripRef}
      data-overflow-start={overflow.start ? "" : undefined}
      data-overflow-end={overflow.end ? "" : undefined}
      style={mask ? { maskImage: mask, WebkitMaskImage: mask } : undefined}
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
    </div>
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
    </>
  );
}
