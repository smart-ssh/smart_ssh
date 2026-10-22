/** Hit-test for native (OS) file drops: `onDragDropEvent` is window-global,
 * so each drop zone checks whether the event position lies inside its own
 * visible element (Spec 0020 §5.4, Spec 0110). The position unit depends on
 * the platform (see `DropPositionUnit`). */
/** Unit of the position a native drag-drop event delivers. Windows and Linux
 * report physical pixels; macOS already reports logical (CSS) pixels, so
 * dividing by the scale factor there would halve the coordinates. */
export type DropPositionUnit = "physical" | "logical";

export function dropPositionUnit(): DropPositionUnit {
  const nav = typeof navigator === "undefined" ? undefined : navigator;
  const platform = `${nav?.platform ?? ""} ${nav?.userAgent ?? ""}`;
  return /mac/i.test(platform) && !/iphone|ipad/i.test(platform)
    ? "logical"
    : "physical";
}

/** Convert a drag-drop event position to CSS pixels. */
export function toCssPosition(
  position: { x: number; y: number },
  scale: number,
  unit: DropPositionUnit,
): { x: number; y: number } {
  if (unit === "logical") return { x: position.x, y: position.y };
  const ratio = scale > 0 ? scale : 1;
  return { x: position.x / ratio, y: position.y / ratio };
}

export function isPointerInside(
  el: HTMLElement | null,
  position: { x: number; y: number } | undefined,
): boolean {
  // A hidden tab/panel has no offsetParent.
  if (!el || el.offsetParent === null || !position) return false;
  const rect = el.getBoundingClientRect();
  const { x, y } = toCssPosition(
    position,
    window.devicePixelRatio || 1,
    dropPositionUnit(),
  );
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}
