import { useLayoutEffect } from "react";
import type { ReactNode } from "react";

import { registerOpenModal } from "../modalLayer";

interface ModalBackdropProps {
  /** Vollständige Klassen des Backdrops (`fixed inset-0 z-50 …`) — bewusst
   * unverändert vom Aufrufer übernommen, damit sich das Aussehen der
   * Dialoge durch diese Komponente nicht ändert. */
  className: string;
  children?: ReactNode;
}

/** Gemeinsamer Backdrop aller modalen Dialoge (Issue #160, Spec 0014,
 * Abschnitt 5). Er sieht aus wie bisher, meldet den Dialog aber beim
 * `modalLayer`-Zähler an — dadurch legt `AppHeader` seine
 * Titelleisten-Drag-Schicht über den Backdrop, und das Fenster bleibt
 * ziehbar, solange der Dialog offen ist. Jeder neue modale Dialog
 * (auch einer, der über die Erweiterungs-Registry beigesteuert wird)
 * nutzt diese Komponente statt eines eigenen `fixed inset-0`-Divs; ein
 * Test (`ModalBackdrop.test.tsx`) prüft das für alle Quelldateien. */
export function ModalBackdrop({ className, children }: ModalBackdropProps) {
  // Layout-Effekt statt `useEffect`: die Drag-Schicht soll im selben
  // Frame erscheinen wie der Backdrop, nicht einen Frame später.
  useLayoutEffect(() => registerOpenModal(), []);

  return (
    <div data-modal-backdrop className={className}>
      {children}
    </div>
  );
}
