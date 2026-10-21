// Issue #578: live media-query match for layout decisions that CSS alone
// cannot express. Reads `matchMedia` synchronously on first render so a
// layout never flashes; without `matchMedia` it reports `false`.
import { useEffect, useState } from "react";

function matches(query: string): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia(query).matches;
}

export function useMediaQuery(query: string): boolean {
  const [value, setValue] = useState(() => matches(query));

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const mql = window.matchMedia(query);
    const update = () => setValue(mql.matches);
    update();
    mql.addEventListener("change", update);
    return () => mql.removeEventListener("change", update);
  }, [query]);

  return value;
}
