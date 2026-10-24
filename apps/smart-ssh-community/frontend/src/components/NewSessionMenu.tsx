// Issue #657: context menu with the single entry "New session", shared by
// the server list and the session tabs.
import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useMenuPlacement } from "../hooks/useMenuPlacement";

export function NewSessionMenu({
  x,
  y,
  serverName,
  onNewSession,
  onClose,
}: {
  x: number;
  y: number;
  serverName: string;
  onNewSession: () => void;
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
      className="fixed z-30 m-0 w-48 min-w-0 border border-slate-700 bg-slate-900 px-0 py-1 text-left text-xs shadow-lg"
      style={placed}
    >
      <legend className="sr-only">{serverName}</legend>
      <button
        ref={buttonRef}
        type="button"
        onClick={onNewSession}
        className="block min-h-11 w-full px-3 py-1.5 text-left text-slate-200 hover:bg-indigo-600/14 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo-400"
      >
        {t("sessionTabs.newSession")}
      </button>
    </fieldset>
  );
}
