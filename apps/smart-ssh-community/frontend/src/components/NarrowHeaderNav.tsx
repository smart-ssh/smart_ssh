// Issue #579, Spec 0113 §3: in the narrow layout the title bar shows a burger
// button and the current title; the burger opens a drawer with the top-level
// navigation and the open sessions. Entries come from the same data as the
// wide layout (`navEntries.ts`, `SessionTabParts.tsx`).
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDialogFocusTrap } from "../useDialogFocusTrap";
import { MAIN_TAB_ENTRIES, type MainTab } from "../navEntries";
import type { SessionTab } from "../useSessionTabs";
import { ModalBackdrop } from "./ModalBackdrop";
import { SessionTabContent, TabStateIndicator, tabDisplayLabel } from "./SessionTabParts";

const FOCUS_RING =
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg focus-visible:ring-offset-1 focus-visible:ring-offset-surface";
const ENTRY_BASE = `${FOCUS_RING} font-heading flex min-h-11 w-full items-center gap-2 border px-3 text-left text-sm font-semibold tracking-wide`;
const entryTone = (current: boolean) =>
  current ? "border-accent/55 bg-accent/16 text-accent-fg-soft" : "border-transparent text-fg-muted hover:bg-surface-raised";

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
      <button
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
        className={`${FOCUS_RING} flex h-11 w-11 shrink-0 items-center justify-center text-fg-soft hover:bg-surface-raised`}
      >
        <svg aria-hidden="true" viewBox="0 0 24 24" className="h-5 w-5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square">
          <path d="M4 6h16M4 12h16M4 18h16" />
        </svg>
      </button>
      <span
        data-tauri-drag-region
        data-testid="narrow-title"
        className="font-heading flex min-w-0 items-center gap-1.5 text-sm font-semibold tracking-wide text-fg"
      >
        {active ? (
          <>
            <TabStateIndicator tab={active} />
            <span className="truncate">{tabDisplayLabel(active, t)}</span>
          </>
        ) : (
          <span className="truncate">Smart SSH</span>
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
  const panelRef = useRef<HTMLDivElement>(null);
  const initialRef = useRef<HTMLButtonElement>(null);
  const { onKeyDown } = useDialogFocusTrap({ containerRef: panelRef, initialFocusRef: initialRef, onEscape: onClose, resetKey: 0 });
  const overview = activeSessionId === null;
  const pick = (action: () => void) => () => {
    action();
    onClose();
  };

  return (
    <ModalBackdrop layer="app" className="fixed inset-0 z-50 bg-overlay/60">
      {/* A tap on the backdrop (outside the panel) closes the menu; Escape is
          handled by the focus trap. */}
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions, jsx-a11y/click-events-have-key-events -- backdrop dismiss; keyboard users use Escape */}
      <div data-testid="narrow-menu-backdrop" className="absolute inset-0" onClick={onClose} />
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Tab handling of the focus trap */}
      <div
        ref={panelRef}
        // eslint-disable-next-line jsx-a11y/prefer-tag-over-role -- a native <dialog> needs showModal() and would fight the shared modal layer
        role="dialog"
        aria-modal="true"
        aria-label={t("sessionTabs.menu.title")}
        onKeyDown={onKeyDown}
        className="absolute inset-y-0 left-0 flex w-[min(20rem,85vw)] flex-col gap-4 overflow-y-auto border-r border-border bg-surface p-3 pt-12"
      >
        <nav aria-label={t("sessionTabs.menu.title")} className="flex flex-col gap-1">
          {MAIN_TAB_ENTRIES.map((entry) => {
            const current = overview && tab === entry.id;
            return (
              <button
                key={entry.id}
                ref={current ? initialRef : undefined}
                type="button"
                aria-current={current ? "page" : undefined}
                onClick={pick(() => onSelectMainTab(entry.id))}
                className={`${ENTRY_BASE} ${entryTone(current)}`}
              >
                {t(entry.labelKey)}
              </button>
            );
          })}
          <button type="button" onClick={pick(onOpenSettings)} className={`${ENTRY_BASE} ${entryTone(false)}`}>
            {t("settings.title")}
          </button>
        </nav>

        {tabs.length > 0 && (
          <section aria-labelledby="narrow-menu-sessions" className="flex flex-col gap-1">
            <h2 id="narrow-menu-sessions" className="px-1 text-xs font-semibold uppercase tracking-wide text-fg-subtle">
              {t("sessionTabs.menu.sessions")}
            </h2>
            <ul className="flex flex-col gap-1">
              {tabs.map((s) => {
                const current = s.sessionId === activeSessionId;
                const label = tabDisplayLabel(s, t);
                return (
                  <li key={s.sessionId} className={`flex items-center border ${current ? "border-accent/55 bg-accent/16" : "border-transparent"}`}>
                    <button
                      ref={!overview && current ? initialRef : undefined}
                      type="button"
                      aria-current={current ? "page" : undefined}
                      onClick={pick(() => onSwitch(s.sessionId))}
                      className={`${FOCUS_RING} font-heading flex min-h-11 min-w-0 flex-1 items-center gap-1.5 px-3 text-left text-sm font-semibold tracking-wide ${current ? "text-accent-fg-soft" : "text-fg-muted"}`}
                    >
                      <SessionTabContent tab={s} t={t} label={label} labelClass="truncate" />
                    </button>
                    {/* Closing keeps the menu open, like closing a tab. */}
                    <button
                      type="button"
                      onClick={() => onRequestClose(s.sessionId)}
                      aria-label={t("sessionTabs.closeTab", { name: label })}
                      className={`${FOCUS_RING} flex h-11 w-11 shrink-0 items-center justify-center text-fg-subtle hover:text-fg-soft`}
                    >
                      ✕
                    </button>
                  </li>
                );
              })}
            </ul>
          </section>
        )}
      </div>
    </ModalBackdrop>
  );
}
