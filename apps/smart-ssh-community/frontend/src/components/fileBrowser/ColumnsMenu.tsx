import { COLUMN_ORDER } from "./shared";
import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { type HideableFileColumn } from "../../layoutSettings";
import { useMenuPlacement } from "../../hooks/useMenuPlacement";

/** Issue #653: right-click menu on the header row — one checkbox per
 * column except Name, which cannot be hidden. */
export function ColumnsMenu({
  point,
  hidden,
  onToggle,
  onClose,
}: {
  point: { x: number; y: number };
  hidden: readonly HideableFileColumn[];
  onToggle: (column: HideableFileColumn) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const menuRef = useRef<HTMLDivElement>(null);
  const getAnchor = useCallback(() => point, [point.x, point.y]); // eslint-disable-line react-hooks/exhaustive-deps -- keyed on the primitive coordinates; the point object is recreated each render
  const style = useMenuPlacement(menuRef, getAnchor, true, { align: "start", gap: 0 });
  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("click", close);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("click", close);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);
  useEffect(() => {
    menuRef.current?.querySelector<HTMLElement>('[role="menuitemcheckbox"]')?.focus();
  }, []);
  return (
    <div
      ref={menuRef}
      role="menu"
      aria-label={t("fileBrowser.columnsMenu")}
      style={style}
      className="fixed z-10 rounded border border-border bg-surface py-1 text-left text-xs shadow-lg"
    >
      {COLUMN_ORDER.map((c) => (
        <button
          key={c}
          type="button"
          role="menuitemcheckbox"
          aria-checked={!hidden.includes(c)}
          onClick={() => onToggle(c)}
          className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-fg-soft hover:bg-accent/14 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-fg"
        >
          <span aria-hidden="true" className="w-3">{hidden.includes(c) ? "" : "✓"}</span>
          {t(`fileBrowser.column.${c}`)}
        </button>
      ))}
    </div>
  );
}
