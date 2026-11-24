// Issue #627: shared placement for context menus, dropdowns and popups.
// Pure geometry, no DOM access, so every edge case is unit-testable.

export interface MenuRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface MenuPlacementInput {
  /** Anchor in viewport coordinates; a point has right = left, bottom = top. */
  anchor: MenuRect;
  /** Measured size of the menu. */
  size: { width: number; height: number };
  viewport: { width: number; height: number };
  /** `start`: left edges line up; `end`: right edges line up. */
  align?: "start" | "end";
  /** Gap kept to the window edges. */
  margin?: number;
  /** Gap between anchor element and menu. */
  gap?: number;
}

export interface MenuPlacement {
  left: number;
  top: number;
  /** Set when the menu is taller than the window: it must scroll. */
  maxHeight: number | null;
}

/** Prefers below/right of the anchor, flips above/left when there is no room,
 * shifts into the window as a last resort and caps the height if needed. */
export function placeMenu({
  anchor,
  size,
  viewport,
  align = "start",
  margin = 8,
  gap = 0,
}: MenuPlacementInput): MenuPlacement {
  const availH = Math.max(0, viewport.height - 2 * margin);
  const availW = Math.max(0, viewport.width - 2 * margin);
  const height = Math.min(size.height, availH);
  const width = Math.min(size.width, availW);

  let top: number;
  const below = anchor.bottom + gap;
  const above = anchor.top - gap - height;
  if (below + height <= viewport.height - margin) top = below;
  else if (above >= margin) top = above;
  else top = Math.max(margin, Math.min(below, viewport.height - margin - height));

  const startLeft = anchor.left;
  const endLeft = anchor.right - width;
  const fits = (l: number) => l >= margin && l + width <= viewport.width - margin;
  const [first, second] = align === "start" ? [startLeft, endLeft] : [endLeft, startLeft];
  let left: number;
  if (fits(first)) left = first;
  else if (fits(second)) left = second;
  else left = Math.max(margin, Math.min(first, viewport.width - margin - width));

  return { left, top, maxHeight: size.height > availH ? availH : null };
}
