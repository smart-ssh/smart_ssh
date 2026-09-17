/** Spec 0057, §4.2 (Etappe 4): "Mache ich selbst" (im Kürzungs-Vorschlags-
 * Dialog, `NoteShrinkSuggestionToast.tsx`, app-weit an der Wurzel von
 * `App.tsx` gemountet — außerhalb von `ManagementView`s eigenem
 * Auswahl-Zustand) muss die Notiz-Bearbeitung für einen BESTIMMTEN Server
 * öffnen können. Dasselbe Bus-Muster wie `extensions/featureLockedBus.ts`
 * (dortiger Doc-Kommentar): eine zentrale Anlaufstelle statt Props quer
 * durch den Baum zu reichen, den `NoteShrinkSuggestionToast` (an der
 * `App`-Wurzel) gar nicht kennt. */

type Listener = (serverId: string) => void;

const listeners = new Set<Listener>();

export function publishRequestServerNoteEdit(serverId: string): void {
  for (const listener of listeners) listener(serverId);
}

/** Gibt eine Unsubscribe-Funktion zurück (React-`useEffect`-Cleanup-Form). */
export function subscribeRequestServerNoteEdit(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
