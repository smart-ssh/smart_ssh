// Issue #657: context menu with the single entry "New session", shared by
// the server list and the session tabs.
import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useMenuPlacement } from "../hooks/useMenuPlacement";

const itemClass =
  "block min-h-11 w-full px-3 py-1.5 text-left text-fg-soft hover:bg-accent/14 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-fg";

export function NewSessionMenu({
  x,
  y,
  serverName,
  onNewSession,
  onKeepAsRegular,
  onClose,
}: {
  x: number;
  y: number;
  serverName: string;
  /** Omitted for a group: the menu then only offers "Keep as regular entry". */
  onNewSession?: () => void;
  /** Spec 0103: only given for an entry that carries the copy marker. */
  onKeepAsRegular?: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLFieldSetElement>(null);
  const getAnchor = useCallback(() => ({ x, y }), [x, y]);
  const placed = useMenuPlacement(menuRef, getAnchor, true);
  useEffect(() => {
    buttonRef.current?.focus();
  }, []);
  useEffect(() => {
    const keyHandler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("click", onClose);
    document.addEventListener("keydown", keyHandler);
    return () => {
      document.removeEventListener("click", onClose);
      document.removeEventListener("keydown", keyHandler);
    };
  }, [onClose]);
  return (
    <fieldset
      ref={menuRef}
      className="fixed z-30 m-0 w-56 min-w-0 border border-border bg-surface px-0 py-1 text-left text-xs shadow-lg"
      style={placed}
    >
      <legend className="sr-only">{serverName}</legend>
      {onNewSession && (
        <button ref={buttonRef} type="button" onClick={onNewSession} className={itemClass}>
          {t("sessionTabs.newSession")}
        </button>
      )}
      {onKeepAsRegular && (
        <button
          ref={onNewSession ? undefined : buttonRef}
          type="button"
          onClick={onKeepAsRegular}
          className={itemClass}
        >
          {t("copyMarker.keep")}
        </button>
      )}
    </fieldset>
  );
}
