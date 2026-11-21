// Issue #579, Spec 0113 §3: in the narrow layout the title bar shows a burger
// button and the current title; the burger opens a drawer with the top-level
// navigation and the open sessions. Entries come from the same data as the
// wide layout (`navEntries.ts`, `SessionTabParts.tsx`).
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { MAIN_TAB_ENTRIES, type MainTab } from "../lib/navEntries";
import type { SessionTab } from "../hooks/useSessionTabs";
import { TabStateIndicator, tabDisplayLabel } from "./SessionTabParts";
import { usePlatform } from "../lib/platform";
import { useActivities } from "../hooks/useActivities";
import { Button, DialogFrame, Icon, Logo, type IconName } from "./ui";

const ENTRY_BASE = `focus-ring label-condensed flex min-h-12 w-full items-center gap-3 border px-4 text-left text-[15px] tracking-[0.08em]`;
const entryTone = (current: boolean) =>
  current ? "border-accent/55 bg-accent/16 text-accent-fg-soft" : "border-transparent text-fg-muted hover:bg-surface-sunken";
const MAIN_ICONS: Record<MainTab, IconName> = { connect: "plug", manage: "server", rules: "shield" };

interface NarrowHeaderNavProps {
  tabs: SessionTab[];
  activeSessionId: string | null;
  tab: MainTab;
  /** Opens a top-level view: leaves any session and selects the tab. */
  onSelectMainTab: (tab: MainTab) => void;
  onSwitch: (sessionId: string | null) => void;
  onRequestClose: (sessionId: string) => void;
  onOpenSettings: (opener: HTMLElement) => void;
}

export function NarrowHeaderNav(props: NarrowHeaderNavProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const burgerRef = useRef<HTMLButtonElement>(null);
  const active = props.tabs.find((x) => x.sessionId === props.activeSessionId);

  return (
    <div data-tauri-drag-region className="flex w-full min-w-0 items-center gap-2">
      <Button variant="bare"
        ref={burgerRef}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={t("sessionTabs.menu.open")}
        // WebKit on macOS does not focus a clicked button; the dialog's
        // focus return needs the burger to be the active element.
        onClick={(e) => {
          e.currentTarget.focus();
          setOpen(true);
        }}
        className="focus-ring flex h-11 w-11 shrink-0 items-center justify-center text-fg-soft hover:bg-surface-raised"
      >
        <svg aria-hidden="true" viewBox="0 0 24 24" className="h-5 w-5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square">
          <path d="M4 6h16M4 12h16M4 18h16" />
        </svg>
      </Button>
      <span
        data-tauri-drag-region
        data-testid="narrow-title"
        className="flex min-w-0 items-center gap-2 font-mono text-sm font-semibold text-fg-strong"
      >
        {active ? (
          <>
            <TabStateIndicator tab={active} />
            <span className="truncate">{tabDisplayLabel(active, t)}</span>
          </>
        ) : (
          <Logo variant="lockup" size={20} />
        )}
      </span>
      {open && (
        <NarrowMenu
          {...props}
          onClose={() => setOpen(false)}
          onOpenSettings={() => props.onOpenSettings(burgerRef.current ?? document.body)}
        />
      )}
    </div>
  );
}

function NarrowMenu({
  tabs,
  activeSessionId,
  tab,
  onSelectMainTab,
  onSwitch,
  onRequestClose,
  onOpenSettings,
  onClose,
}: Omit<NarrowHeaderNavProps, "onOpenSettings"> & { onOpenSettings: () => void; onClose: () => void }) {
  const { t } = useTranslation();
  const initialRef = useRef<HTMLButtonElement>(null);
  const overview = activeSessionId === null;
  const pick = (action: () => void) => () => {
    action();
    onClose();
  };

  // Desktop windows keep the title bar (drag layer) free above the sheet; Android has none.
  const topGap = usePlatform() === "android" ? "" : " pt-14";
  const activities = useActivities();
  const running = activities.filter((a) => a.state === "running").length;
  const openActivities = () => {
    // The activity center is the popover of the header icon; the entry opens that one.
    document.querySelector<HTMLElement>('[data-testid="activity-center-button"]')?.click();
  };

  return (
    <DialogFrame
      layer="app"
      layout="sheet"
      bare
      ariaLabel={t("sessionTabs.menu.title")}
      onClose={onClose}
      onBackdropClick={onClose}
      initialFocusRef={initialRef}
      panelClassName={`absolute inset-y-0 left-0 flex w-[min(330px,85vw)] flex-col overflow-y-auto border-r border-border bg-surface-raised${topGap}`}
    >
      <div className="flex h-14 shrink-0 items-center justify-between border-b border-border pl-4 pr-1">
        <Logo variant="lockup" size={20} />
        <Button
          variant="bare"
          type="button"
          aria-label={t("sessionTabs.menu.close")}
          onClick={onClose}
          className="focus-ring flex h-12 w-12 items-center justify-center text-fg-soft hover:bg-surface-sunken"
        >
          <Icon name="x" size={20} />
        </Button>
      </div>
      <nav aria-label={t("sessionTabs.menu.title")} className="flex flex-col border-b border-border py-2">
        {MAIN_TAB_ENTRIES.map((entry) => {
          const current = overview && tab === entry.id;
          return (
            <Button
              variant="bare"
              key={entry.id}
              ref={current ? initialRef : undefined}
              type="button"
              aria-current={current ? "page" : undefined}
              onClick={pick(() => onSelectMainTab(entry.id))}
              className={`${ENTRY_BASE} ${entryTone(current)}`}
            >
              <Icon name={MAIN_ICONS[entry.id]} size={20} />
              {t(entry.labelKey)}
            </Button>
          );
        })}
      </nav>

      <div className="flex flex-1 flex-col gap-1 px-2 py-3">
        {tabs.length > 0 && (
          <section aria-labelledby="narrow-menu-sessions" className="flex flex-col gap-1">
            <h2 id="narrow-menu-sessions" className="text-label px-2 pb-1">
              {t("sessionTabs.menu.sessions")}
            </h2>
            <ul className="flex flex-col gap-1">
              {tabs.map((s) => {
                const current = s.sessionId === activeSessionId;
                const label = tabDisplayLabel(s, t);
                return (
                  <li key={s.sessionId} className={`flex items-center border ${current ? "border-accent/55 bg-accent/16" : "border-transparent"}`}>
                    <Button
                      variant="bare"
                      ref={!overview && current ? initialRef : undefined}
                      type="button"
                      aria-current={current ? "page" : undefined}
                      onClick={pick(() => onSwitch(s.sessionId))}
                      className={`focus-ring flex min-h-12 min-w-0 flex-1 items-center gap-2 px-3 text-left font-mono text-sm font-semibold ${current ? "text-accent-fg-soft" : "text-fg"}`}
                    >
                      <TabStateIndicator tab={s} />
                      {s.mcp && (
                        <span className="shrink-0 border border-local/60 px-1 text-[10px] leading-tight text-local-fg">
                          {t("sessionTabs.mcp.badge")}
                        </span>
                      )}
                      <span title={label} className="truncate">
                        {label}
                      </span>
                      {s.hasPendingAction && (
                        <span className="label-condensed shrink-0 border border-warning/60 px-1.5 text-[10px] leading-4 text-warning-fg">
                          {t("sessionTabs.menu.needsConfirmation")}
                        </span>
                      )}
                    </Button>
                    {/* Closing keeps the menu open, like closing a tab. */}
                    <Button
                      variant="bare"
                      type="button"
                      onClick={() => onRequestClose(s.sessionId)}
                      aria-label={t("sessionTabs.closeTab", { name: label })}
                      className="focus-ring flex h-12 w-12 shrink-0 items-center justify-center text-fg-subtle hover:text-fg-soft"
                    >
                      <Icon name="x" />
                    </Button>
                  </li>
                );
              })}
            </ul>
          </section>
        )}
        <Button
          variant="bare"
          type="button"
          onClick={pick(() => onSelectMainTab("connect"))}
          className={`${ENTRY_BASE} ${entryTone(false)} text-fg-subtle`}
        >
          <Icon name="plus" size={20} />
          {t("sessionTabs.newSession")}
        </Button>
      </div>

      <div className="flex shrink-0 flex-col border-t border-border py-2">
        {activities.length > 0 && (
          <Button variant="bare" type="button" onClick={pick(openActivities)} className={`${ENTRY_BASE} ${entryTone(false)}`}>
            <Icon name="activity" size={20} />
            <span className="flex-1">{t("sessionTabs.menu.activities")}</span>
            {running > 0 && (
              <span className="text-[11px] tracking-[0.1em] text-accent-fg-soft">
                {t("sessionTabs.menu.activitiesRunning", { count: running })}
              </span>
            )}
          </Button>
        )}
        <Button variant="bare" type="button" onClick={pick(onOpenSettings)} className={`${ENTRY_BASE} ${entryTone(false)}`}>
          <Icon name="sliders" size={20} />
          {t("settings.title")}
        </Button>
      </div>
    </DialogFrame>
  );
}
