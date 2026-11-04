import { useDragResize } from "../../hooks/useDragResize";

/** Spec 0053, Teil 1: Drag-Handle am rechten Rand eines Spaltenkopfs — die
 * Trefferfläche (`w-2`, per `-right-1` über die Spaltengrenze zentriert)
 * ist bewusst breiter als die sichtbare Trennlinie (`w-px`), damit sie
 * auch ohne Pixel-genaues Zielen zu treffen ist. `cursor-col-resize` /
 * `touch-none` (kein Scroll-Gestenkonflikt beim Ziehen) als der von der
 * Spec verlangte Hover-/Cursor-Hinweis.
 *
 * Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): kein
 * `role="separator"`/`aria-label` mehr — ein per Tastatur nicht
 * bedienbares, rein visuelles Maus-Element (die Spec verlangt
 * Tastaturbedienbarkeit ausdrücklich nicht) würde Screenreadern beim
 * Durchlaufen der Tabellenkopfzeile als benanntes, aber funktionsloses
 * Element angesagt — irreführend. `aria-hidden` ist hier die ehrlichere
 * Wahl; `data-testid` bleibt als reiner Test-Anker. */
export function ColumnResizeHandle({
  testId,
  onDrag,
  onDragEnd,
}: {
  testId: string;
  onDrag: (deltaX: number) => void;
  onDragEnd: () => void;
}) {
  const handlers = useDragResize(onDrag, onDragEnd);
  return (
    <span
      aria-hidden="true"
      data-testid={testId}
      className="group absolute -right-1 top-0 bottom-0 z-10 flex w-2 cursor-col-resize touch-none select-none justify-center"
      {...handlers}
    >
      <span className="h-full w-px bg-border group-hover:bg-accent-strong" />
    </span>
  );
}
