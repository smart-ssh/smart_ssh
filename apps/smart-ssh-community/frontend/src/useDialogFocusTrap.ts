import { useEffect, useRef, type KeyboardEvent, type RefObject } from "react";

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

  // A7: Fokus vor dem Öffnen merken, beim Unmount zurückgeben (sofern das
  // Element noch existiert — ein zwischenzeitlich entferntes Element lässt
  // sich nicht mehr fokussieren).
  useEffect(() => {
    previouslyFocusedRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => {
      const previous = previouslyFocusedRef.current;
      if (previous && document.contains(previous)) {
        previous.focus();
      }
    };
    // Bewusst nur beim Mount/Unmount — `previouslyFocusedRef` soll den
    // Fokus vor dem allerersten Öffnen festhalten, nicht bei jedem
    // Re-Render neu einlesen.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // A2: Anfangsfokus beim Mount und bei jedem `resetKey`-Wechsel.
  useEffect(() => {
    initialFocusRef.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [resetKey]);

  // A3: Fokus, der den Dialog auf andere Weise als Tab verlässt (Klick auf
  // den Hintergrund, `blur()`, programmatischer Fokus auf ein Element
  // außerhalb), wird zurückgeholt. Zwei Fälle, **gemessen** in `jsdom`
  // (Spec 0100 §1) und deshalb bewusst zwei Mechanismen statt einem:
  //
  // - Ein `blur()` ohne nächstes Ziel fällt auf `body` zurück und feuert
  //   **nur** ein `focusout` (mit `relatedTarget` `null`) — kein `focusin`
  //   folgt. Dafür reicht ein synchroner Re-Fokus in `focusout`.
  // - Ein direkter `.focus()` auf ein Element außerhalb ist eine zweite,
  //   konkurrierende Fokus-Operation: ein synchroner Re-Fokus **innerhalb**
  //   des dazu gehörenden `focusout` wird von dieser noch laufenden
  //   Operation überschrieben (in `jsdom` wie im echten Browser derselbe
  //   Mechanismus) — der Re-Fokus muss im nachfolgenden `focusin` auf dem
  //   neuen (externen) Ziel passieren, wenn die konkurrierende Operation
  //   bereits abgeschlossen ist.
  //
  // Auf `document` statt auf dem Container gehängt, weil ein nach
  // `document.body` geportalter Dialog Events nur über dessen natives
  // Bubbling empfängt, nicht über den React-Baum, in dem der Container-Ref
  // hängt — und weil die Rückgabe so **jeden** Fokusverlust abfängt, nicht
  // nur einen, der innerhalb des portalten Teilbaums auftritt.
  useEffect(() => {
    function isOutside(target: EventTarget | null): boolean {
      const container = containerRef.current;
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
    document.addEventListener("focusout", onFocusOut);
    document.addEventListener("focusin", onFocusIn);
    return () => {
      document.removeEventListener("focusout", onFocusOut);
      document.removeEventListener("focusin", onFocusIn);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function onKeyDown(event: KeyboardEvent<HTMLElement>) {
    if (event.key === "Escape") {
      // A4: nicht an dahinterliegende Handler weiterreichen (z. B. die
      // globalen Kürzel in `App.tsx`).
      event.preventDefault();
      event.stopPropagation();
      onEscape();
      return;
    }
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
