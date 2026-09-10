import { useCallback, useRef } from "react";

/** Spec 0053: gemeinsame Drag-Logik für die Spalten-Handles im
 * Dateimanager (`FileBrowserPanel`) und den KI-/SSH-Bereichs-Splitter
 * (`SessionView`) — Pointer Events mit `setPointerCapture` statt
 * globaler `document`-Listener: der Browser liefert Move-/Up-Events
 * weiter an das ziehende Element, auch wenn der Zeiger währenddessen das
 * Element verlässt (schnelles Ziehen) oder über eine andere, eigentlich
 * hover-reagierende Fläche läuft — ohne dass wir Listener manuell
 * an-/abmelden und uns um Aufräumen bei einem vorzeitig beendeten Drag
 * kümmern müssten.
 *
 * `onDrag` bekommt nur das Delta seit dem letzten Move-Event (nicht seit
 * Drag-Start) — jeder Aufrufer akkumuliert selbst auf seinem eigenen
 * State, das hält diesen Hook unabhängig davon, wie viele Werte eine
 * Bewegung betrifft (z. B. eine Spaltenbreite direkt plus die
 * Name-Spalte indirekt als Rest).
 *
 * `onDragEnd` läuft nur, wenn während der Geste tatsächlich eine
 * Bewegung stattfand (nicht bei einem bloßen Klick ohne Ziehen) — für
 * die Persistenz-Schreiboperation, die nicht bei jedem Pixel, nur einmal
 * am Ende einer abgeschlossenen Geste passieren soll. */
export function useDragResize(onDrag: (deltaX: number) => void, onDragEnd?: () => void) {
  const draggingRef = useRef(false);
  const lastXRef = useRef(0);
  const movedRef = useRef(false);

  const onPointerDown = useCallback((event: React.PointerEvent) => {
    event.preventDefault();
    draggingRef.current = true;
    movedRef.current = false;
    lastXRef.current = event.clientX;
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
  }, []);

  const onPointerMove = useCallback(
    (event: React.PointerEvent) => {
      if (!draggingRef.current) return;
      const delta = event.clientX - lastXRef.current;
      if (delta === 0) return;
      lastXRef.current = event.clientX;
      movedRef.current = true;
      onDrag(delta);
    },
    [onDrag],
  );

  const onPointerUp = useCallback(
    (event: React.PointerEvent) => {
      if (!draggingRef.current) return;
      draggingRef.current = false;
      (event.currentTarget as Element).releasePointerCapture(event.pointerId);
      if (movedRef.current) onDragEnd?.();
    },
    [onDragEnd],
  );

  return { onPointerDown, onPointerMove, onPointerUp };
}
