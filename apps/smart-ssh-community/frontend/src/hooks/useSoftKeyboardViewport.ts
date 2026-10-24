// Issue #678, Spec 0114: while the soft keyboard is open, the app root
// follows the visual viewport (`--app-vh`), so the terminal area shrinks and
// the cursor line stays visible; closing the keyboard restores full height.
import { useEffect } from "react";

export function useSoftKeyboardViewport(active: boolean): void {
  useEffect(() => {
    const vv = typeof window !== "undefined" ? window.visualViewport : null;
    if (!active || !vv) return;
    const root = document.documentElement;
    const update = () => {
      const covered = window.innerHeight - vv.height - vv.offsetTop;
      if (covered > 80) root.style.setProperty("--app-vh", `${vv.height}px`);
      else root.style.removeProperty("--app-vh");
    };
    update();
    vv.addEventListener("resize", update);
    vv.addEventListener("scroll", update);
    return () => {
      vv.removeEventListener("resize", update);
      vv.removeEventListener("scroll", update);
      root.style.removeProperty("--app-vh");
    };
  }, [active]);
}
