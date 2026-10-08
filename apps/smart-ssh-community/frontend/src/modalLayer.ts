import { useSyncExternalStore } from "react";

/** Issue #160 / Spec 0014, Abschnitt 5: verfolgt, ob gerade ein modaler
 * Dialog **sichtbar** geöffnet ist. Jede `ModalBackdrop`-Instanz meldet ihr
 * Backdrop-Element beim Mounten an und beim Unmounten ab; `AppHeader` liest
 * daraus, ob seine Titelleisten-Drag-Schicht über den Backdrops liegen muss.
 *
 * Bewusst ein Modul-Speicher statt eines React-Contexts: Dialoge werden teils
 * per Portal nach `document.body` gerendert und teils über die
 * Erweiterungs-Registry beigesteuert — beides liegt nicht zuverlässig unter
 * einem gemeinsamen Provider.
 *
 * Gezählt wird nur ein Backdrop, der tatsächlich angezeigt wird: Inaktive
 * Session-Tabs und die Startansicht bleiben gemountet und werden nur per
 * `display:none` (`hidden`) ausgeblendet, ebenso Teilbereiche einer
 * Session. Ein inline (ohne Portal) gerenderter Dialog darin — etwa die
 * Einstellungen oder ein Dateibrowser-Dialog nach einem Tab-Wechsel per
 * Tastatur — ist dann unsichtbar und darf die Header-Inhalte nicht
 * blockieren. Da ein solcher Wechsel nur eine Klasse an einem Vorfahren
 * ändert und den Dialog selbst nicht berührt, beobachtet ein
 * `MutationObserver` Klassen-, Style- und `hidden`-Änderungen im Dokument,
 * solange mindestens ein Backdrop angemeldet ist, und prüft die
 * Sichtbarkeit dann neu. */

/** z-Index aller modalen Backdrops (`z-50`). */
export const MODAL_BACKDROP_Z_INDEX = 50;

/** z-Index der Titelleisten-Drag-Schicht: über jedem modalen Backdrop,
 * aber unter den Fenster-Controls von `tauri-plugin-decoration`
 * (`--tauri-plugin-decoration-z-index: 100` in `index.css`), damit
 * Minimieren/Maximieren/Schließen unter Windows/Linux klickbar bleiben.
 * Wer einen Dialog mit höherem z-Index einführt, muss diesen Wert
 * mitziehen — sonst deckt der Dialog die Titelleiste wieder ab. */
export const TITLEBAR_DRAG_LAYER_Z_INDEX = 60;

/** Angemeldete Backdrops; ein Element kann (StrictMode, zwei Effekte)
 * mehrfach angemeldet sein, daher mit Zähler. */
const registered = new Map<HTMLElement, number>();
const listeners = new Set<() => void>();
let anyVisible = false;
let observer: MutationObserver | null = null;

/** `true`, wenn das Element im Dokument hängt und weder es selbst noch ein
 * Vorfahre `display:none` hat. */
function isDisplayed(el: HTMLElement): boolean {
  if (!el.isConnected) return false;
  const view = el.ownerDocument.defaultView;
  if (!view) return false;
  for (let node: Element | null = el; node; node = node.parentElement) {
    if (view.getComputedStyle(node).display === "none") return false;
  }
  return true;
}

function recompute() {
  let next = false;
  for (const el of registered.keys()) {
    if (isDisplayed(el)) {
      next = true;
      break;
    }
  }
  if (next === anyVisible) return;
  anyVisible = next;
  for (const listener of listeners) listener();
}

function startObserving() {
  if (observer || typeof MutationObserver === "undefined") return;
  observer = new MutationObserver(recompute);
  observer.observe(document.documentElement, {
    subtree: true,
    attributes: true,
    attributeFilter: ["class", "style", "hidden"],
  });
}

function stopObserving() {
  observer?.disconnect();
  observer = null;
}

/** Meldet das Backdrop-Element eines geöffneten Modals an. Rückgabe:
 * Abmelde-Funktion (idempotent, mehrfacher Aufruf zählt nur einmal). */
export function registerOpenModal(backdrop: HTMLElement): () => void {
  registered.set(backdrop, (registered.get(backdrop) ?? 0) + 1);
  startObserving();
  recompute();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    const count = (registered.get(backdrop) ?? 1) - 1;
    if (count > 0) registered.set(backdrop, count);
    else registered.delete(backdrop);
    if (registered.size === 0) stopObserving();
    recompute();
  };
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot() {
  return anyVisible;
}

/** `true`, solange mindestens ein modaler Dialog geöffnet und sichtbar ist. */
export function useIsAnyModalOpen(): boolean {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
