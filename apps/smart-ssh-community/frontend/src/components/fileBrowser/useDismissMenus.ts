import { useEffect } from "react";
import type { RemoteEntryDto } from "../../types";
import { focusMenuTrigger } from "./shared";

type OpenMenu = {
  entry: RemoteEntryDto;
  anchor: "row" | { x: number; y: number };
} | null;
type Point = { x: number; y: number } | null;

/** Click outside / Escape close the entry menu and the empty-space menu. */
export function useDismissMenus(
  openMenu: OpenMenu,
  setOpenMenu: (menu: OpenMenu) => void,
  dirMenu: Point,
  setDirMenu: (menu: Point) => void,
) {
  // Schließt das offene Menü (Drei-Punkte oder Kontextmenü) bei einem Klick
  // irgendwo sonst hin. Die Trigger (⋮-Button, Rechtsklick) rufen jeweils
  // `stopPropagation()` auf (s. dortige Kommentare, Spec 0054 Teil 0) —
  // dieser Listener sieht also nie den Klick, der ein Menü gerade erst
  // geöffnet/gewechselt hat, nur einen tatsächlich "von außen" kommenden.
  useEffect(() => {
    if (openMenu === null) return;
    const handler = () => setOpenMenu(null);
    // Escape closes the menu too; focus goes back to the ⋮ button that
    // opened it (docs/ui-guidelines.md, Keyboard).
    const keyHandler = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const closing = openMenu;
      setOpenMenu(null);
      if (closing.anchor === "row") {
        focusMenuTrigger(closing.entry.path);
      }
    };
    document.addEventListener("click", handler);
    document.addEventListener("keydown", keyHandler);
    return () => {
      document.removeEventListener("click", handler);
      document.removeEventListener("keydown", keyHandler);
    };
  }, [openMenu]);

  // Issue #570: Kontextmenü auf leerer Fläche — Klick außerhalb und Escape
  // schließen es wie das Eintragsmenü.
  useEffect(() => {
    if (dirMenu === null) return;
    const handler = () => setDirMenu(null);
    const keyHandler = (e: KeyboardEvent) => {
      if (e.key === "Escape") setDirMenu(null);
    };
    document.addEventListener("click", handler);
    document.addEventListener("keydown", keyHandler);
    return () => {
      document.removeEventListener("click", handler);
      document.removeEventListener("keydown", keyHandler);
    };
  }, [dirMenu]);
}
