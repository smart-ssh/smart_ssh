// Issue #540: follow new content in a scroll container only while the user
// is at the bottom; once they scroll away, leave the position alone.
import { type RefObject, useCallback, useLayoutEffect, useRef, useState } from "react";

/** Same threshold as `LiveCommandOutput`. */
export const STICK_THRESHOLD_PX = 16;

export function useStickToBottom(
  scrollRef: RefObject<HTMLElement | null>,
  contentRef: RefObject<HTMLElement | null>,
  dep: unknown,
) {
  const followRef = useRef(true);
  const [atBottom, setAtBottom] = useState(true);

  const pin = useCallback(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight; // instant, never smooth
  }, [scrollRef]);

  const onScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    // A programmatic pin ends at the bottom, so it can only confirm "follow".
    const near = el.scrollHeight - el.scrollTop - el.clientHeight <= STICK_THRESHOLD_PX;
    followRef.current = near;
    setAtBottom(near);
  }, [scrollRef]);

  /** Scrolls to the bottom and resumes following. */
  const scrollToBottom = useCallback(() => {
    followRef.current = true;
    setAtBottom(true);
    pin();
  }, [pin]);

  // Before paint: no flash of the old position.
  useLayoutEffect(() => {
    if (followRef.current) pin();
  }, [dep, pin]);

  // Height changes that do not go through `dep` (live output, images, …).
  useLayoutEffect(() => {
    const content = contentRef.current;
    if (!content || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (followRef.current) pin();
    });
    observer.observe(content);
    return () => observer.disconnect();
  }, [contentRef, pin]);

  return { atBottom, onScroll, scrollToBottom };
}
