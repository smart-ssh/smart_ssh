// Issue #627: places a `position: fixed` menu fully inside the window.
// Use it for every new menu, dropdown or popup (see menuPlacement.ts).
import { type CSSProperties, type RefObject, useCallback, useLayoutEffect, useRef, useState } from "react";
import { type MenuRect, placeMenu } from "../menuPlacement";
import { useIsNarrow } from "./useIsNarrow";

/** A viewport point, an element, or a function returning either (or null). */
export type MenuAnchor = { x: number; y: number } | Element | null;

function toRect(anchor: MenuAnchor): MenuRect | null {
  if (anchor === null) return null;
  if (anchor instanceof Element) {
    const r = anchor.getBoundingClientRect();
    return { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
  }
  return { left: anchor.x, top: anchor.y, right: anchor.x, bottom: anchor.y };
}

/**
 * Returns the style for the menu element `menuRef` (give it `fixed` in CSS).
 * Measures before paint and again on window resize while `active`. In the
 * narrow layout the menus are bottom sheets, so no style is returned.
 */
export function useMenuPlacement(
  menuRef: RefObject<HTMLElement | null>,
  getAnchor: () => MenuAnchor,
  active: boolean,
  options: { align?: "start" | "end"; gap?: number } = {},
): CSSProperties | undefined {
  const narrow = useIsNarrow();
  const { align, gap } = options;
  // No `visibility: hidden` for the first frame: the layout effect below places
  // the menu before paint, and a hidden menu could not take focus on mount.
  const [style, setStyle] = useState<CSSProperties>({ left: 0, top: 0 });
  const anchorRef = useRef(getAnchor);
  const place = useCallback(() => {
    const menu = menuRef.current;
    const anchor = toRect(anchorRef.current());
    if (!menu || !anchor) return;
    // scrollHeight is the natural height even while max-height clips it.
    const p = placeMenu({
      anchor,
      size: { width: menu.offsetWidth, height: Math.max(menu.offsetHeight, menu.scrollHeight) },
      viewport: { width: window.innerWidth, height: window.innerHeight },
      align,
      gap,
    });
    const next: CSSProperties = {
      left: p.left,
      top: p.top,
      ...(p.maxHeight !== null ? { maxHeight: p.maxHeight, overflowY: "auto" as const } : {}),
    };
    setStyle((prev) =>
      prev.left === next.left && prev.top === next.top && prev.maxHeight === next.maxHeight
        ? prev
        : next,
    );
  }, [menuRef, align, gap]);
  // Every render: the anchor point or the menu content may have changed.
  // `setStyle` bails out when nothing moved, so this does not loop.
  useLayoutEffect(() => {
    anchorRef.current = getAnchor;
    if (active && !narrow) place();
  });
  useLayoutEffect(() => {
    if (!active || narrow) return;
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [active, narrow, place]);
  return narrow ? undefined : style;
}
