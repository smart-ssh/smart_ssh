import { useCallback, useEffect, useRef, useState } from "react";
import { sessionIsRoot } from "../../api/api";
import { useIsNarrow } from "../../hooks/useIsNarrow";
import { useMenuPlacement } from "../../hooks/useMenuPlacement";

interface UseToolbarMenusOptions {
  isVisible: boolean;
  elevation: unknown;
  elevationMenuOpen: boolean;
  otherUserOpen: boolean;
  setElevationMenuOpen: (open: boolean) => void;
  setOtherUserOpen: (open: boolean) => void;
}

/**
 * Open/close behaviour, focus and placement of the toolbar menus (overflow
 * menu, elevation menu, target-user form) and the `id -u` probe of the session.
 */
export function useToolbarMenus(
  sessionId: string,
  {
    isVisible,
    elevation,
    elevationMenuOpen,
    otherUserOpen,
    setElevationMenuOpen,
    setOtherUserOpen,
  }: UseToolbarMenusOptions,
) {
  // Issue #544: `id -u` probe result of the session. `undefined` = not yet
  // answered; `null`/failure = unknown (controls shown); `true` = root (hidden).
  const [sessionRoot, setSessionRoot] = useState<boolean | null | undefined>(undefined);
  // Issue #584, Spec 0113: below 640 px the toolbar collapses into an
  // overflow menu and the list into two-line rows; functions stay the same.
  const narrow = useIsNarrow();
  const [overflowOpen, setOverflowOpen] = useState(false);
  const overflowRef = useRef<HTMLDivElement>(null);
  const overflowButtonRef = useRef<HTMLButtonElement>(null);
  const elevationMenuButtonRef = useRef<HTMLButtonElement>(null);
  const otherUserInputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    let cancelled = false;
    sessionIsRoot(sessionId)
      .then((value) => {
        if (!cancelled) setSessionRoot(value);
      })
      .catch(() => {
        if (!cancelled) setSessionRoot(null);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId]);
  const otherUserItemRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!elevationMenuOpen && !otherUserOpen) return;
    if (elevationMenuOpen) otherUserItemRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      setElevationMenuOpen(false);
      setOtherUserOpen(false);
      (elevationMenuButtonRef.current ?? overflowButtonRef.current)?.focus();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [elevationMenuOpen, otherUserOpen, setElevationMenuOpen, setOtherUserOpen]);
  const elevationGroupRef = useRef<HTMLFieldSetElement>(null);
  // Issue #627: the elevation menu and the target-user form stay inside the window.
  const elevationMenuRef = useRef<HTMLDivElement>(null);
  const otherUserFormRef = useRef<HTMLFormElement>(null);
  const getElevationAnchor = useCallback(() => elevationGroupRef.current, []);
  const elevationMenuStyle = useMenuPlacement(elevationMenuRef, getElevationAnchor, elevationMenuOpen, { gap: 4 });
  const otherUserFormStyle = useMenuPlacement(otherUserFormRef, getElevationAnchor, otherUserOpen, { gap: 4 });
  // Issue #569: a click outside the elevation group closes the menu and the
  // target user form. Clicks inside (arrow button, menu, form) are ignored, so
  // the arrow button's own toggle is not undone.
  useEffect(() => {
    if (!elevationMenuOpen && !otherUserOpen) return;
    const onClick = (e: MouseEvent) => {
      // composedPath, not target: a clicked menu item may already be detached.
      const path = e.composedPath();
      const group = elevationGroupRef.current;
      if (group && path.includes(group)) return;
      const overflow = overflowRef.current;
      if (overflow && path.includes(overflow)) return;
      setElevationMenuOpen(false);
      setOtherUserOpen(false);
    };
    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, [elevationMenuOpen, otherUserOpen, setElevationMenuOpen, setOtherUserOpen]);
  // Hiding the panel or switching the elevation closes both.
  useEffect(() => {
    setElevationMenuOpen(false);
    setOtherUserOpen(false);
    setOverflowOpen(false);
  }, [isVisible, elevation, setElevationMenuOpen, setOtherUserOpen]);
  // Leaving the narrow layout drops the overflow menu.
  useEffect(() => {
    if (!narrow) setOverflowOpen(false);
  }, [narrow]);
  // Overflow menu: outside click and Escape close it (Escape returns focus).
  useEffect(() => {
    if (!overflowOpen) return;
    const onClick = (e: MouseEvent) => {
      const overflow = overflowRef.current;
      if (overflow && e.composedPath().includes(overflow)) return;
      setOverflowOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      setOverflowOpen(false);
      overflowButtonRef.current?.focus();
    };
    document.addEventListener("click", onClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [overflowOpen]);

  return {
    sessionRoot,
    narrow,
    overflowOpen,
    setOverflowOpen,
    overflowRef,
    overflowButtonRef,
    elevationMenuButtonRef,
    otherUserInputRef,
    otherUserItemRef,
    elevationGroupRef,
    elevationMenuRef,
    otherUserFormRef,
    elevationMenuStyle,
    otherUserFormStyle,
  };
}
