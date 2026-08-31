import { useEffect, useState } from "react";
import { commandErrorMessage, respondToAction } from "../api";
import { onNoteUpdateSuggested } from "../events";
import type { NoteUpdateSuggestedEvent } from "../types";

interface PendingSuggestion {
  sessionId: string;
  actionId: string;
  target: { kind: "server" | "group"; id: string };
  newContent: string;
  /** Kompakte Ansicht per Default (Spec 0010, Abschnitt 2, Punkt 6: "dezente
   * Benachrichtigung statt eines blockierenden Modals") — erst nach Klick
   * auf "Anzeigen" wird der Inhalt (das "Dialog"-Äquivalent) eingeblendet. */
  expanded: boolean;
  deciding: boolean;
  error: string | null;
}

function targetFromEvent(event: NoteUpdateSuggestedEvent): { kind: "server" | "group"; id: string } {
  const target = event.action.ProposeNoteUpdate.target;
  return "Server" in target
    ? { kind: "server", id: target.Server }
    : { kind: "group", id: target.Group };
}

/**
 * Spec 0010, Abschnitt 2, Punkt 5/6: App-weite, nicht-blockierende
 * Benachrichtigung für einen KI-Notiz-Vorschlag nach `disconnect()` — kann
 * eintreffen, nachdem der Nutzer den Session-Screen bereits verlassen hat,
 * deshalb hier auf App-Ebene (nicht innerhalb eines bestimmten Screens)
 * gerendert. Design-Entscheidung (Spec lässt die genaue Darstellung offen):
 * eine kompakte Toast-Karte unten rechts, die sich beim Klick auf
 * "Anzeigen" in derselben Karte zum vollen Vorschlag (Ziel + neuer Inhalt +
 * Annehmen/Ablehnen) aufklappt, statt ein separates Modal über den
 * aktuellen Screen zu legen — genau das schließt die Spec ausdrücklich aus
 * ("statt eines blockierenden Modals"). Siehe ADR-Vorschlag am Ende der
 * Aufgabe.
 */
export function NoteSuggestionToast() {
  const [suggestions, setSuggestions] = useState<PendingSuggestion[]>([]);

  useEffect(() => {
    const unlisten = onNoteUpdateSuggested((event) => {
      setSuggestions((prev) => [
        ...prev,
        {
          sessionId: event.sessionId,
          actionId: event.actionId,
          target: targetFromEvent(event),
          newContent: event.action.ProposeNoteUpdate.new_content,
          expanded: false,
          deciding: false,
          error: null,
        },
      ]);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const updateSuggestion = (actionId: string, patch: Partial<PendingSuggestion>) => {
    setSuggestions((prev) => prev.map((s) => (s.actionId === actionId ? { ...s, ...patch } : s)));
  };

  const dismiss = (actionId: string) => {
    setSuggestions((prev) => prev.filter((s) => s.actionId !== actionId));
  };

  const decide = async (suggestion: PendingSuggestion, decision: "approve" | "deny") => {
    updateSuggestion(suggestion.actionId, { deciding: true, error: null });
    try {
      await respondToAction(suggestion.sessionId, suggestion.actionId, { decision });
      dismiss(suggestion.actionId);
    } catch (err) {
      updateSuggestion(suggestion.actionId, {
        deciding: false,
        error: commandErrorMessage(err),
      });
    }
  };

  if (suggestions.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2">
      {suggestions.map((suggestion) => (
        <div
          key={suggestion.actionId}
          className="rounded-lg border border-slate-700 bg-slate-800 p-3 text-sm shadow-lg"
        >
          {!suggestion.expanded ? (
            <div className="flex items-center justify-between gap-2">
              <span className="text-slate-200">
                KI schlägt eine Notiz-Aktualisierung vor (
                {suggestion.target.kind === "server" ? "Server" : "Gruppe"})
              </span>
              <div className="flex shrink-0 gap-1">
                <button
                  type="button"
                  onClick={() => updateSuggestion(suggestion.actionId, { expanded: true })}
                  className="rounded bg-indigo-600 px-2 py-1 text-xs text-white hover:bg-indigo-500"
                >
                  Anzeigen
                </button>
                <button
                  type="button"
                  onClick={() => decide(suggestion, "deny")}
                  className="rounded bg-slate-700 px-2 py-1 text-xs hover:bg-slate-600"
                  aria-label="Vorschlag verwerfen"
                >
                  ✕
                </button>
              </div>
            </div>
          ) : (
            <div className="space-y-2">
              <p className="text-xs uppercase tracking-wide text-slate-400">
                Notiz-Vorschlag ({suggestion.target.kind === "server" ? "Server" : "Gruppe"}{" "}
                {suggestion.target.id})
              </p>
              <pre className="max-h-40 overflow-y-auto whitespace-pre-wrap rounded bg-slate-950 p-2 text-xs text-slate-300">
                {suggestion.newContent}
              </pre>
              {suggestion.error && <p className="text-xs text-red-400">{suggestion.error}</p>}
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => decide(suggestion, "deny")}
                  disabled={suggestion.deciding}
                  className="rounded bg-red-900 px-3 py-1 text-xs text-red-200 hover:bg-red-800 disabled:opacity-50"
                >
                  Ablehnen
                </button>
                <button
                  type="button"
                  onClick={() => decide(suggestion, "approve")}
                  disabled={suggestion.deciding}
                  className="rounded bg-emerald-700 px-3 py-1 text-xs text-white hover:bg-emerald-600 disabled:opacity-50"
                >
                  {suggestion.deciding ? "Übernimmt…" : "Übernehmen"}
                </button>
              </div>
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
