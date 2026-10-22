// Issue #578, Spec 0113: the single JS definition of the narrow breakpoint.
// The CSS side is `@custom-variant narrow` in `index.css` (`narrow:` prefix);
// both must use the same query.
import { useMediaQuery } from "./useMediaQuery";

/** First wide width in px; windows below it use the narrow layout. */
export const NARROW_BREAKPOINT_PX = 640;
/** Fractional max-width so that 640 px itself is wide, also at odd zoom levels. */
export const NARROW_QUERY = `(max-width: ${NARROW_BREAKPOINT_PX - 0.02}px)`;

export function useIsNarrow(): boolean {
  return useMediaQuery(NARROW_QUERY);
}
