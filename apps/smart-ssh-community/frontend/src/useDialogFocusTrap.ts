import { useEffect, useRef, type KeyboardEvent as ReactKeyboardEvent, type RefObject } from "react";

const FOCUSABLE_SELECTOR =
  'a[href], area[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function getFocusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
}

interface UseDialogFocusTrapOptions {
  /** Das Element, das den Fokus-Fang trägt (Rolle `dialog`/`alertdialog`). */
  containerRef: RefObject<HTMLElement | null>;
  /** Die Schaltfläche, die beim Öffnen und bei jedem `resetKey`-Wechsel den
   * Fokus bekommt — bei Host-Key-Dialogen die ablehnende Schaltfläche. */
  initialFocusRef: RefObject<HTMLElement | null>;
  /** Wird bei Escape gerufen; der Aufrufer entscheidet, was das bedeutet
   * (hier: ablehnen) und schützt selbst vor Mehrfachaufruf. */
  onEscape: () => void;
  /** Ändert sich dieser Wert (z. B. ein neues Ereignis oder ein Zweigwechsel
   * unbekannt → geändert), wird der Anfangsfokus erneut gesetzt — ein
   * Re-Render darf den Fokus nie auf der riskanten Schaltfläche oder dem
   * Hintergrund belassen (Spec 0100, A2, Testfall "rerender"). */
  resetKey: unknown;
}

/**
 * Spec 0100: Fokus-Fang, Anfangsfokus und Fokus-Rückgabe für einen per
 * `createPortal` an `document.body` gehängten modalen Dialog. Tab und
 * Shift+Tab werden vollständig selbst im `keydown` behandelt (nicht nur an
 * den Rändern) — `jsdom` führt eine native Tab-Bewegung nicht aus, und ein
 * selbst geführter Fokus-Fang ist auch im echten Browser der übliche Weg,
 * um zuverlässig zu verhindern, dass der Fokus den Dialog verlässt.
 *
 * Reiner Hook ohne eigenes Markup — rendert nichts, portalt also auch
 * nichts selbst; ein Aufrufer, der eine eigene Dialog-Hülle daraus baut,
 * entscheidet selbst über deren Portal-Ziel (vgl. Spec 0100, Abschnitt 3,
 * zum Verhältnis zu BL-0162).
 */
export function useDialogFocusTrap({
  containerRef,
  initialFocusRef,
  onEscape,
  resetKey,
}: UseDialogFocusTrapOptions) {
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);

  // A3/A4/A7, alle in einem Effekt mit `[]`-Abhängigkeiten, bewusst nicht
  // getrennt (Review-Fund, Spec 0100): Getrennte Effekte hätten keine
  // erzwungene Reihenfolge zwischen „Fang abmelden" und „Fokus zurückgeben"
  // — und der Fang (`onFocusIn` unten) würde einen Fokus, der durch A7s
  // Rückgabe auf ein Element außerhalb des Containers wandert, grundsätzlich
  // als Fokusverlust werten und sofort zurückholen. In einem Effekt ist die
  // Reihenfolge erzwungen: zuerst die Listener abmelden, erst danach den
  // Fokus zurückgeben, sodass der Fang die Rückgabe nicht mehr sehen kann.
  //
  // Muss außerdem **vor** dem A2-Effekt (Anfangsfokus, unten) deklariert
  // sein: Effekte laufen beim Mount in Deklarationsreihenfolge, und die
  // Erfassung von `previouslyFocusedRef` hier liest `document.activeElement`
  // — liefe A2 schon vorher, wäre das bereits die eigene `reject`-
  // Schaltfläche statt des Elements, das vor dem Öffnen den Fokus trug, und
  // A7 gäbe am Ende den Fokus nicht an den richtigen Ort zurück (gemessen:
  // mit vertauschter Reihenfolge scheitert T10 zuverlässig).
  useEffect(() => {
    previouslyFocusedRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;

    function isOutside(target: EventTarget | null): boolean {
      const container = containerRef.current;
      // `false` bei fehlendem Container (z. B. kurz vor dem eigenen
      // Unmount) ist Absicht, nicht Nachlässigkeit: Andernfalls würde ein
      // nach dem Entfernen des Containers noch eintreffendes Fokus-Ereignis
      // fälschlich als „außerhalb" gelten und erneut `initialFocusRef`
      // fokussieren — ein Fokus-Pingpong, der genau die A7-Rückgabe direkt
      // darunter wieder zunichtemachen würde.
      if (!container) return false;
      return !(target instanceof Node) || !container.contains(target);
    }
    function onFocusOut(event: FocusEvent) {
      if (event.relatedTarget) return; // folgt ein focusin, s. onFocusIn
      initialFocusRef.current?.focus();
    }
    function onFocusIn(event: FocusEvent) {
      if (isOutside(event.target)) {
        initialFocusRef.current?.focus();
      }
    }
    // A4: Escape auf `document` statt nur im Container-`onKeyDown` unten,
    // damit es auch dann ablehnt und nicht durchgereicht wird, wenn der
    // Fokus gerade *nicht* im Dialog liegt — etwa weil das fokussierte
    // Element entfernt wurde und der Fokus (ohne eigenes Ereignis, je nach
    // Engine) auf `document.body` zurückgefallen ist (Review-Fund, Spec
    // 0100). Auf `document` registriert läuft dieser Handler vor jedem
    // Listener auf `window` (z. B. die Kürzel in `App.tsx`), `document`
    // liegt in der Bubble-Kette näher am Ursprung.
    function onDocumentKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onEscape();
    }
    document.addEventListener("focusout", onFocusOut);
    document.addEventListener("focusin", onFocusIn);
    document.addEventListener("keydown", onDocumentKeyDown);
    return () => {
      document.removeEventListener("focusout", onFocusOut);
      document.removeEventListener("focusin", onFocusIn);
      document.removeEventListener("keydown", onDocumentKeyDown);
      // A7: erst nachdem der Fang abgemeldet ist (s. Kommentar oben) —
      // sonst holt er sich den Fokus sofort zurück.
      const previous = previouslyFocusedRef.current;
      if (previous && document.contains(previous)) {
        previous.focus();
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // A2: Anfangsfokus beim Mount und bei jedem `resetKey`-Wechsel. Muss
  // **nach** dem Effekt oben deklariert sein, s. dessen Kommentar.
  useEffect(() => {
    initialFocusRef.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [resetKey]);

  function onKeyDown(event: ReactKeyboardEvent<HTMLElement>) {
    if (event.key === "Enter" && event.repeat) {
      // A5: eine aus einem vorherigen Dialog gehaltene Enter-Taste darf die
      // gerade erst fokussierte Schaltfläche nicht auslösen.
      event.preventDefault();
      return;
    }
    if (event.key === "Tab") {
      const container = containerRef.current;
      if (!container) return;
      const focusable = getFocusableElements(container);
      if (focusable.length === 0) return;
      event.preventDefault();
      const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
      const delta = event.shiftKey ? -1 : 1;
      const nextIndex = (currentIndex + delta + focusable.length) % focusable.length;
      focusable[nextIndex]?.focus();
    }
  }

  return { onKeyDown };
}
