/** Hit-test for native (OS) file drops: `onDragDropEvent` is window-global,
 * so each drop zone checks whether the event position lies inside its own
 * visible element (Spec 0020 §5.4, Spec 0110). Positions are physical pixels;
 * dividing by `devicePixelRatio` yields CSS pixels. */
export function isPointerInside(
  el: HTMLElement | null,
  position: { x: number; y: number } | undefined,
): boolean {
  // A hidden tab/panel has no offsetParent.
  if (!el || el.offsetParent === null || !position) return false;
  const rect = el.getBoundingClientRect();
  const ratio = window.devicePixelRatio || 1;
  const x = position.x / ratio;
  const y = position.y / ratio;
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}
