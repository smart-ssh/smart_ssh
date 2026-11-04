import {
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
  useEffect,
  useRef,
} from "react";
import { type MenuAnchor, useMenuPlacement } from "../../hooks/useMenuPlacement";

const ITEM_SELECTOR = '[role^="menuitem"]:not(:disabled)';

export interface MenuProps {
  /** Accessible name of the menu. */
  label: string;
  /** A viewport point or an element to anchor to (or a function returning one). */
  anchor: MenuAnchor | (() => MenuAnchor);
  /** Called on Escape, Tab, a click outside and after a `menuitem` was activated. */
  onClose: () => void;
  /** Element that gets focus back on Escape; defaults to what was focused on open. */
  returnFocusTo?: () => Element | null;
  /** Bottom-sheet layout (narrow window): larger touch targets, full width. */
  narrow?: boolean;
  align?: "start" | "end";
  gap?: number;
  /** Width / z-index classes of the wide layout. */
  className?: string;
  children: ReactNode;
}

/**
 * Context or dropdown menu (role `menu`). Arrow up/down/Home/End move focus
 * between the enabled items, Escape closes and returns focus to the opener,
 * Tab closes, a click outside closes. Placement: `useMenuPlacement`. A click
 * on a `menuitem` closes the menu after the item's own handler; a
 * `menuitemcheckbox` keeps it open.
 */
export function Menu({
  label,
  anchor,
  onClose,
  returnFocusTo,
  narrow = false,
  align = "start",
  gap = 0,
  className = "z-20 w-52",
  children,
}: MenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const openerRef = useRef<Element | null>(null);
  const onCloseRef = useRef(onClose);
  const returnRef = useRef(returnFocusTo);
  useEffect(() => {
    onCloseRef.current = onClose;
    returnRef.current = returnFocusTo;
  });
  const getAnchor = () => (typeof anchor === "function" ? anchor() : anchor);
  const style = useMenuPlacement(menuRef, getAnchor, true, { align, gap });

  useEffect(() => {
    openerRef.current = document.activeElement;
    menuRef.current?.querySelector<HTMLElement>(ITEM_SELECTOR)?.focus();
  }, []);

  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      const target = e.target as Element | null;
      const menu = menuRef.current;
      if (menu?.contains(target)) {
        const item = target?.closest('[role="menuitem"]');
        if (item && !(item as HTMLButtonElement).disabled) onCloseRef.current();
        return;
      }
      onCloseRef.current();
    };
    // Escape is heard on the document so it also works when focus left the menu.
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const target = returnRef.current?.() ?? openerRef.current;
      onCloseRef.current();
      if (target instanceof HTMLElement && target.isConnected) target.focus();
    };
    document.addEventListener("click", onClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKey);
    };
  }, []);

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(menuRef.current?.querySelectorAll<HTMLElement>(ITEM_SELECTOR) ?? []);
    const idx = items.indexOf(document.activeElement as HTMLElement);
    let next: number | null = null;
    switch (e.key) {
      case "ArrowDown":
        next = idx < 0 ? 0 : (idx + 1) % items.length;
        break;
      case "ArrowUp":
        next = idx < 0 ? items.length - 1 : (idx - 1 + items.length) % items.length;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = items.length - 1;
        break;
      case "Tab":
        onCloseRef.current();
        return;
      default:
        return;
    }
    e.preventDefault();
    if (items.length > 0 && next !== null) items[next].focus();
  };

  return (
    // The menu role is exposed on the container; keys are handled for the items inside.
    // oxlint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
    <div
      ref={menuRef}
      role="menu"
      tabIndex={-1}
      aria-label={label}
      data-narrow={narrow ? "true" : undefined}
      onKeyDown={onKeyDown}
      className={
        narrow
          ? "fixed inset-x-0 bottom-0 z-30 max-h-[70vh] overflow-y-auto rounded-t border border-border bg-surface py-1 text-left text-xs shadow-lg"
          : `fixed ${className} border border-border bg-surface py-1 text-left text-xs shadow-lg`
      }
      style={narrow ? undefined : style}
    >
      {children}
    </div>
  );
}

export interface MenuItemProps {
  onClick?: () => void;
  icon?: ReactNode;
  /** Shortcut hint, shown right-aligned. */
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  /** Larger touch target (bottom sheet). */
  narrow?: boolean;
  /** Makes the item a toggle (`menuitemcheckbox`) with this state; it keeps the menu open. */
  checked?: boolean;
  title?: string;
  children: ReactNode;
}

export function MenuItem({
  onClick,
  icon,
  shortcut,
  disabled,
  danger,
  narrow,
  checked,
  title,
  children,
}: MenuItemProps) {
  const tone = danger
    ? "text-danger-fg hover:bg-danger/12"
    : "text-fg-soft hover:bg-accent/14";
  return (
    <button
      type="button"
      role={checked === undefined ? "menuitem" : "menuitemcheckbox"}
      aria-checked={checked}
      disabled={disabled}
      title={title}
      onClick={onClick}
      className={`flex w-full items-center gap-2 px-3 ${narrow ? "min-h-11 py-2 text-sm" : "py-1.5"} text-left ${tone} focus-ring-inset disabled:cursor-not-allowed disabled:opacity-50`}
    >
      {icon !== undefined && <span aria-hidden="true" className="w-3">{icon}</span>}
      <span className="flex-1">{children}</span>
      {shortcut && <span className="text-fg-muted">{shortcut}</span>}
    </button>
  );
}

export function MenuSeparator() {
  return <hr className="my-1 border-0 border-t border-surface-raised" />;
}
