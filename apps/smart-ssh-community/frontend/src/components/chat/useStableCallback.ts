import { useCallback, useLayoutEffect, useRef } from "react";

/**
 * Returns a function whose identity never changes but which always calls the
 * latest `fn`. Lets the chat panel hand the same callbacks to every memoised
 * `ChatItemView`, so a streamed text delta re-renders only the changed item.
 */
export function useStableCallback<A extends unknown[], R>(fn: (...args: A) => R): (...args: A) => R {
  const ref = useRef(fn);
  useLayoutEffect(() => {
    ref.current = fn;
  });
  return useCallback((...args: A) => ref.current(...args), []);
}
