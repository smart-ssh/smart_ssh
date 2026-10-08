import { useSyncExternalStore } from "react";

/** Issue #160 / Spec 0014, Abschnitt 5: zählt die gerade geöffneten
 * modalen Dialoge. Jede `ModalBackdrop`-Instanz meldet sich beim Mounten an
 * und beim Unmounten ab; `AppHeader` liest daraus, ob seine
 * Titelleisten-Drag-Schicht über den Backdrops liegen muss.
 *
 * Bewusst ein Modul-Zähler statt eines React-Contexts: Dialoge werden teils
 * per Portal nach `document.body` gerendert und teils über die
 * Erweiterungs-Registry beigesteuert — beides liegt nicht zuverlässig unter
 * einem gemeinsamen Provider. */

/** z-Index aller modalen Backdrops (`z-50`). */
export const MODAL_BACKDROP_Z_INDEX = 50;

/** z-Index der Titelleisten-Drag-Schicht: über jedem modalen Backdrop,
 * aber unter den Fenster-Controls von `tauri-plugin-decoration`
 * (`--tauri-plugin-decoration-z-index: 100` in `index.css`), damit
 * Minimieren/Maximieren/Schließen unter Windows/Linux klickbar bleiben.
 * Wer einen Dialog mit höherem z-Index einführt, muss diesen Wert
 * mitziehen — sonst deckt der Dialog die Titelleiste wieder ab. */
export const TITLEBAR_DRAG_LAYER_Z_INDEX = 60;

let openModalCount = 0;
const listeners = new Set<() => void>();

function notify() {
  for (const listener of listeners) listener();
}

/** Meldet einen geöffneten Modal an. Rückgabe: Abmelde-Funktion
 * (idempotent, mehrfacher Aufruf zählt nur einmal). */
export function registerOpenModal(): () => void {
  openModalCount += 1;
  notify();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    openModalCount -= 1;
    notify();
  };
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot() {
  return openModalCount > 0;
}

/** `true`, solange mindestens ein modaler Dialog geöffnet ist. */
export function useIsAnyModalOpen(): boolean {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
