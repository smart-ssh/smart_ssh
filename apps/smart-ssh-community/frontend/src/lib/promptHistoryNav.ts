// Reine Navigationslogik für die Pfeiltasten-Historie im Chat-Eingabefeld
// (Spec 0015, Abschnitt 5) — bewusst getrennt von `ChatPanel.tsx`s
// DOM-Cursor-Abfragen/Event-Handling, damit sie ohne Browser-Umgebung
// testbar ist (s. `promptHistoryNav.test.ts`). `ChatPanel.tsx` bestimmt nur,
// *ob* eine Pfeiltaste die Navigation auslösen darf (Cursor-Position-Gate),
// und ruft dann `navigateHistory` für das eigentliche "welcher Eintrag als
// Nächstes" auf.

export interface HistoryNavState {
  /** `null` = nicht im Navigations-Modus (Feld zeigt den Entwurf). Sonst
   * Index in `history` (chronologisch aufsteigend, wie `list_prompt_history`
   * liefert) des aktuell angezeigten Eintrags. */
  historyIndex: number | null;
  /** Nur relevant, während `historyIndex !== null` — der Entwurfstext, der
   * beim ersten Pfeil-nach-oben zwischengespeichert wurde (auch wenn leer). */
  stashedDraft: string;
}

export const initialHistoryNavState: HistoryNavState = {
  historyIndex: null,
  stashedDraft: "",
};

export type ArrowDirection = "up" | "down";

export interface HistoryNavResult {
  /** Der Text, der jetzt im Eingabefeld stehen soll. */
  value: string;
  nextState: HistoryNavState;
}

/**
 * Berechnet den nächsten Navigationsschritt (Spec 0015, Abschnitt 5).
 * `history` chronologisch aufsteigend, jüngster Eintrag also
 * `history[history.length - 1]`. Gibt `null` zurück, wenn sich für die
 * gegebene Richtung nichts ändert — z. B. Pfeil-nach-oben beim bereits
 * ältesten Eintrag (dort "bleibt es stehen, keine Fehlermeldung"), oder
 * Pfeil-nach-unten außerhalb des Navigations-Modus (dann gilt normale
 * Cursor-Bewegung, nicht Sache dieser Funktion).
 */
export function navigateHistory(
  direction: ArrowDirection,
  history: string[],
  state: HistoryNavState,
  currentDraft: string,
): HistoryNavResult | null {
  if (direction === "up") {
    if (history.length === 0) return null;

    if (state.historyIndex === null) {
      const nextIndex = history.length - 1;
      return {
        value: history[nextIndex],
        nextState: { historyIndex: nextIndex, stashedDraft: currentDraft },
      };
    }
    if (state.historyIndex > 0) {
      const nextIndex = state.historyIndex - 1;
      return {
        value: history[nextIndex],
        nextState: { ...state, historyIndex: nextIndex },
      };
    }
    return null;
  }

  // direction === "down"
  if (state.historyIndex === null) return null;

  if (state.historyIndex < history.length - 1) {
    const nextIndex = state.historyIndex + 1;
    return {
      value: history[nextIndex],
      nextState: { ...state, historyIndex: nextIndex },
    };
  }

  return {
    value: state.stashedDraft,
    nextState: { historyIndex: null, stashedDraft: "" },
  };
}
