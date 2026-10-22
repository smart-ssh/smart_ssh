import { useId, type KeyboardEvent, type ReactNode } from "react";

export interface TabItem {
  id: string;
  label: ReactNode;
  /** Marks the tab as containing an error (shown as a red dot + hidden text). */
  invalid?: boolean;
  /** Screen-reader text appended to an invalid tab. */
  invalidLabel?: string;
}

/**
 * Accessible tab bar (WAI-ARIA tabs, automatic activation). One Tab stop for the
 * whole bar; Left/Right (Up/Down) move between tabs, Home/End jump to the ends.
 * Panels are rendered by the caller with {@link TabPanel}, so that they can stay
 * mounted (hidden) and keep their state across tab switches.
 */
export function Tabs({
  tabs,
  value,
  onChange,
  ariaLabel,
  idBase,
}: {
  tabs: TabItem[];
  value: string;
  onChange: (id: string) => void;
  ariaLabel: string;
  /** Same base as passed to the matching {@link TabPanel}s. */
  idBase: string;
}) {
  const handleKeyDown = (
    e: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    let next = index;
    if (e.key === "ArrowRight" || e.key === "ArrowDown")
      next = (index + 1) % tabs.length;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp")
      next = (index - 1 + tabs.length) % tabs.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = tabs.length - 1;
    else return;
    e.preventDefault();
    onChange(tabs[next].id);
    document.getElementById(tabId(idBase, tabs[next].id))?.focus();
  };

  return (
    <div
      role="tablist"
      aria-label={ariaLabel}
      className="flex gap-1 border-b border-slate-700"
    >
      {tabs.map((tab, index) => {
        const selected = tab.id === value;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            id={tabId(idBase, tab.id)}
            aria-selected={selected}
            aria-controls={panelId(idBase, tab.id)}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(tab.id)}
            onKeyDown={(e) => handleKeyDown(e, index)}
            className={`-mb-px rounded-t border-b-2 px-3 py-1.5 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 ${
              selected
                ? "border-indigo-400 text-slate-100"
                : "border-transparent text-slate-300 hover:text-white"
            }`}
          >
            {tab.label}
            {tab.invalid && (
              <>
                <span
                  aria-hidden="true"
                  className="ml-1.5 inline-block h-2 w-2 rounded-full bg-red-400"
                />
                {tab.invalidLabel && (
                  <span className="sr-only"> ({tab.invalidLabel})</span>
                )}
              </>
            )}
          </button>
        );
      })}
    </div>
  );
}

export function TabPanel({
  idBase,
  id,
  active,
  children,
  className,
}: {
  idBase: string;
  id: string;
  active: boolean;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      role="tabpanel"
      id={panelId(idBase, id)}
      aria-labelledby={tabId(idBase, id)}
      hidden={!active}
      className={className}
    >
      {children}
    </div>
  );
}

/** Stable id base for one tab group. */
export function useTabsIdBase(): string {
  return useId();
}

export const tabId = (base: string, id: string) => `${base}-tab-${id}`;
const panelId = (base: string, id: string) => `${base}-panel-${id}`;
