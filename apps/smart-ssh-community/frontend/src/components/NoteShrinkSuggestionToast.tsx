import { useEffect, useState } from "react";
import { commandErrorMessage, requestNoteShrink } from "../api";
import { onNoteShrinkFailed, onNoteShrinkSuggested } from "../events";
import { publishRequestServerNoteEdit } from "../navigationBus";

interface PendingShrinkSuggestion {
  serverId: string;
  serverName: string;
}

interface ShrinkFailure {
  id: string;
  message: string;
}

/**
 * Spec 0057, §4.2 (Etappe 4): der ERSTE Dialog beim Verbindungsende, wenn
 * die gespeicherte Notiz eines Servers groß ist — "Ja, zusammenfassen" /
 * "Mache ich selbst". App-weit gemountet (wie `NoteSuggestionToast`,
 * dieselbe Begründung: kann eintreffen, nachdem der Nutzer den
 * Session-Screen bereits verlassen hat).
 *
 * Design-Entscheidung: klickt der Nutzer "Ja, zusammenfassen", wird DIESE
 * Karte sofort verworfen (der eigentliche KI-Aufruf läuft danach im
 * Hintergrund, s. `commands::request_note_shrink`) — der tatsächliche
 * Diff-Bestätigungsdialog kommt über das bereits bestehende
 * `note-update-suggested`-Event/`NoteSuggestionToast` an, keine zweite,
 * parallele Diff-UI. Ein Fehlschlag des Hintergrund-Aufrufs (`note-shrink-
 * failed`) wird stattdessen als eigene, kurze Fehler-Karte gezeigt — deren
 * Nachricht bereits vom Backend nutzerfreundlich formuliert ist ("… Die
 * gespeicherte Notiz wurde nicht verändert.").
 */
export function NoteShrinkSuggestionToast() {
  const [suggestions, setSuggestions] = useState<PendingShrinkSuggestion[]>([]);
  const [failures, setFailures] = useState<ShrinkFailure[]>([]);

  useEffect(() => {
    const unlistenSuggested = onNoteShrinkSuggested((event) => {
      setSuggestions((prev) => [
        ...prev,
        { serverId: event.serverId, serverName: event.serverName },
      ]);
    });
    const unlistenFailed = onNoteShrinkFailed((event) => {
      setFailures((prev) => [...prev, { id: crypto.randomUUID(), message: event.message }]);
    });
    return () => {
      unlistenSuggested.then((fn) => fn());
      unlistenFailed.then((fn) => fn());
    };
  }, []);

  const dismiss = (serverId: string) => {
    setSuggestions((prev) => prev.filter((s) => s.serverId !== serverId));
  };

  const dismissFailure = (id: string) => {
    setFailures((prev) => prev.filter((f) => f.id !== id));
  };

  const summarize = async (suggestion: PendingShrinkSuggestion) => {
    dismiss(suggestion.serverId);
    try {
      await requestNoteShrink(suggestion.serverId);
    } catch (err) {
      setFailures((prev) => [
        ...prev,
        { id: crypto.randomUUID(), message: commandErrorMessage(err) },
      ]);
    }
  };

  const editMyself = (suggestion: PendingShrinkSuggestion) => {
    dismiss(suggestion.serverId);
    publishRequestServerNoteEdit(suggestion.serverId);
  };

  if (suggestions.length === 0 && failures.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2">
      {suggestions.map((suggestion) => (
        <div
          key={suggestion.serverId}
          className="rounded-lg border border-slate-700 bg-slate-800 p-3 text-sm shadow-lg"
        >
          <p className="font-semibold text-slate-100">
            Notiz für Server „{suggestion.serverName}“ ist sehr groß
          </p>
          <p className="mt-1 text-xs text-slate-400">
            Deine Notiz für diesen Server ist sehr groß und kann bei langen Sitzungen gekürzt
            werden müssen. Soll ich sie zusammenfassen?
          </p>
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              onClick={() => editMyself(suggestion)}
              className="rounded bg-slate-700 px-3 py-1 text-xs hover:bg-slate-600"
            >
              Mache ich selbst
            </button>
            <button
              type="button"
              onClick={() => summarize(suggestion)}
              className="rounded bg-indigo-600 px-3 py-1 text-xs text-white hover:bg-indigo-500"
            >
              Ja, zusammenfassen
            </button>
          </div>
        </div>
      ))}
      {failures.map((failure) => (
        <div
          key={failure.id}
          className="flex items-start justify-between gap-2 rounded-lg border border-red-900 bg-red-950 p-3 text-sm shadow-lg"
        >
          <p className="text-red-200">{failure.message}</p>
          <button
            type="button"
            onClick={() => dismissFailure(failure.id)}
            className="shrink-0 rounded bg-red-900 px-2 py-1 text-xs text-red-200 hover:bg-red-800"
            aria-label="Fehlermeldung verwerfen"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
