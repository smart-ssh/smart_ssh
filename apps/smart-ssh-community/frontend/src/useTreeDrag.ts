import { useCallback, useEffect, useRef, useState } from "react";
import { DROP_TARGET_ATTRIBUTE, parseDropTarget, type DragItem, type DropTarget } from "./treeDrag";

/** Ab dieser Zeigerbewegung (in px) wird aus einem Klick ein Ziehen —
 * darunter bleibt es ein normaler Klick (Verbinden, Auswählen,
 * Auf-/Zuklappen). */
export const DRAG_THRESHOLD_PX = 5;

export interface TreeDragState {
  item: DragItem;
  /** Ablageziel unter dem Zeiger, `null` = kein gültiges Ziel. */
  target: DropTarget | null;
  x: number;
  y: number;
}

/** Ablageziel unter einem Bildschirmpunkt: das innerste Element mit
 * `data-drop-target`. Ein Element mit `data-drop-target="none"` schirmt
 * seine Vorfahren ab (kein Ziel). */
export function dropTargetAt(x: number, y: number): DropTarget | null {
  // `elementFromPoint` fehlt in jsdom — dort liefert es schlicht kein Ziel.
  const element = typeof document.elementFromPoint === "function"
    ? document.elementFromPoint(x, y)
    : null;
  const holder = element?.closest(`[${DROP_TARGET_ATTRIBUTE}]`);
  return parseDropTarget(holder?.getAttribute(DROP_TARGET_ATTRIBUTE));
}

interface Pending {
  item: DragItem;
  pointerId: number;
  startX: number;
  startY: number;
  element: Element;
}

/**
 * Issue #48 / Spec 0103: Drag-and-drop in den Server-Bäumen über Pointer
 * Events, **nicht** über HTML5-Drag-and-drop. Tauris native Dateiablage
 * (`onDragDropEvent`, Dateibrowser) kann HTML5-Drag-Events im Webview
 * abfangen — unter Windows kommen sie dann gar nicht an. Pointer Events
 * sind davon unabhängig und verhalten sich auf allen drei Plattformen
 * gleich (dasselbe Muster wie `useDragResize.ts`).
 *
 * Ablauf: `pointerdown` merkt sich den Kandidaten; erst ab
 * [`DRAG_THRESHOLD_PX`] Bewegung beginnt das Ziehen (mit
 * `setPointerCapture`, damit Move/Up auch außerhalb der Zeile ankommen).
 * Das Ziel unter dem Zeiger ermittelt [`dropTargetAt`]. Beim Loslassen
 * ruft der Hook `onDrop(item, target)` — nur mit einem Ziel; ob das Ziel
 * für dieses Element sinnvoll ist, entscheidet der Aufrufer
 * (`classifyDrop`). Escape oder ein abgebrochener Zeiger beenden das
 * Ziehen ohne Ablage. Der Klick, den der Browser nach einem Ziehen noch
 * auf das Ausgangselement liefert, wird verschluckt, damit ein Ziehen
 * nicht zusätzlich verbindet oder auswählt.
 */
export function useTreeDrag(onDrop: (item: DragItem, target: DropTarget) => void) {
  const [drag, setDrag] = useState<TreeDragState | null>(null);
  const pendingRef = useRef<Pending | null>(null);
  const draggingRef = useRef(false);
  const targetRef = useRef<DropTarget | null>(null);
  const suppressClickRef = useRef(false);
  const onDropRef = useRef(onDrop);
  useEffect(() => {
    onDropRef.current = onDrop;
  }, [onDrop]);

  const reset = useCallback(() => {
    const pending = pendingRef.current;
    if (pending && draggingRef.current) {
      try {
        pending.element.releasePointerCapture?.(pending.pointerId);
      } catch {
        // Capture schon vom Browser gelöst — nichts zu tun.
      }
    }
    pendingRef.current = null;
    draggingRef.current = false;
    targetRef.current = null;
    setDrag(null);
  }, []);

  useEffect(() => {
    if (!drag) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        // Der folgende Klick (falls der Zeiger noch losgelassen wird)
        // darf nichts auslösen.
        suppressClickRef.current = true;
        reset();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [drag, reset]);

  const handlersFor = useCallback(
    (item: DragItem) => ({
      onPointerDown: (event: React.PointerEvent) => {
        suppressClickRef.current = false;
        if (event.button !== 0) return;
        pendingRef.current = {
          item,
          pointerId: event.pointerId,
          startX: event.clientX,
          startY: event.clientY,
          element: event.currentTarget,
        };
      },
      onPointerMove: (event: React.PointerEvent) => {
        const pending = pendingRef.current;
        if (!pending || pending.pointerId !== event.pointerId) return;
        if (event.buttons === 0) {
          // Ein vom System verschluckter `pointerup` (s. `useDragResize`).
          reset();
          return;
        }
        if (!draggingRef.current) {
          const dx = event.clientX - pending.startX;
          const dy = event.clientY - pending.startY;
          if (Math.hypot(dx, dy) < DRAG_THRESHOLD_PX) return;
          draggingRef.current = true;
          try {
            pending.element.setPointerCapture?.(event.pointerId);
          } catch {
            // Ohne Capture funktioniert das Ziehen weiterhin, solange der
            // Zeiger über dem Baum bleibt.
          }
        }
        const target = dropTargetAt(event.clientX, event.clientY);
        targetRef.current = target;
        setDrag({ item: pending.item, target, x: event.clientX, y: event.clientY });
      },
      onPointerUp: (event: React.PointerEvent) => {
        const pending = pendingRef.current;
        if (!pending || pending.pointerId !== event.pointerId) return;
        const wasDragging = draggingRef.current;
        const target = targetRef.current;
        reset();
        if (!wasDragging) return;
        suppressClickRef.current = true;
        if (target) onDropRef.current(pending.item, target);
      },
      onPointerCancel: () => {
        reset();
      },
      onClickCapture: (event: React.MouseEvent) => {
        if (suppressClickRef.current) {
          suppressClickRef.current = false;
          event.preventDefault();
          event.stopPropagation();
        }
      },
    }),
    [reset],
  );

  return { drag, handlersFor };
}
