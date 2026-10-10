import { useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";

import { registerOpenModal } from "../modalLayer";

/** `"app"`: per `createPortal` an `document.body`; `"sessionTab"`: an Ort
 * und Stelle, verschwindet mit seinem Tab. */
export type ModalBackdropLayer = "app" | "sessionTab";

interface ModalBackdropProps {
  /** Vollständige Klassen des Backdrops (`fixed inset-0 z-50 …`) — bewusst
   * unverändert vom Aufrufer übernommen, damit sich das Aussehen der
   * Dialoge durch diese Komponente nicht ändert. */
  className: string;
  /** Zu welcher Ebene der Dialog gehört (Issue #244): `"app"` für Dialoge
   * der ganzen App, `"sessionTab"` für Dialoge eines Session-Tabs. */
  layer: ModalBackdropLayer;
  children?: ReactNode;
}

/** Gemeinsamer Backdrop aller modalen Dialoge (Issue #160, Spec 0014,
 * Abschnitt 5). Er sieht aus wie bisher, meldet den Dialog aber beim
 * `modalLayer`-Zähler an — dadurch legt `AppHeader` seine
 * Titelleisten-Drag-Schicht über den Backdrop, und das Fenster bleibt
 * ziehbar, solange der Dialog offen ist. Jeder neue modale Dialog
 * (auch einer, der über die Erweiterungs-Registry beigesteuert wird)
 * nutzt diese Komponente statt eines eigenen `fixed inset-0`-Divs; ein
 * Test (`TitleBarDragLayer.test.tsx`) prüft das für alle Quelldateien.
 *
 * `layer` ist Pflicht: Dialoge der ganzen App (Einstellungen,
 * Host-Key-Abfrage, Start-Dialoge) nutzen `"app"` und werden nach
 * `document.body` gerendert, damit ein per `display:none` ausgeblendeter
 * Vorfahr (inaktiver Session-Tab, Startansicht) sie nicht unsichtbar macht.
 * Nur Dialoge, die mit ihrem Session-Tab verschwinden sollen, nutzen
 * `"sessionTab"`; sie bleiben an Ort und Stelle. Eigene `createPortal`-
 * Aufrufe für Backdrops gehören nicht in Dialoge. */
export function ModalBackdrop({ className, layer, children }: ModalBackdropProps) {
  const ref = useRef<HTMLDivElement>(null);
  // Layout-Effekt statt `useEffect`: die Drag-Schicht soll im selben
  // Frame erscheinen wie der Backdrop, nicht einen Frame später. Das
  // Element wird mit angemeldet, damit `modalLayer` nur einen tatsächlich
  // angezeigten Backdrop zählt (nicht einen in einem per `display:none`
  // ausgeblendeten Session-Tab).
  useLayoutEffect(() => (ref.current ? registerOpenModal(ref.current) : undefined), []);

  const backdrop = (
    <div ref={ref} data-modal-backdrop className={className}>
      {children}
    </div>
  );
  return layer === "app" ? createPortal(backdrop, document.body) : backdrop;
}
