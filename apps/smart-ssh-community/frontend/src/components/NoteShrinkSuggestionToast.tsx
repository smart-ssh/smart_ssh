import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, requestNoteShrink } from "../api";
import { onNoteShrinkFailed, onNoteShrinkSuggested } from "../events";
import { publishRequestServerNoteEdit } from "../navigationBus";
import { Button } from "./ui";

interface PendingShrinkSuggestion {
  serverId: string;
  serverName: string;
}

interface ShrinkFailure {
  id: string;
  message: string;
}

/**
 * Spec 0079, A2/A3: Server-IDs, für die "Später" geklickt wurde — bewusst
 * eine Modul-Variable statt Komponenten-`State`, damit sie ein Neu-Einhängen
 * von `NoteShrinkSuggestionToast` überlebt (die Komponente ist app-weit
 * gemountet, s. Moduldoc oben, ein Neu-Mount ist also kein alltäglicher
 * Fall, aber Tests hängen z. B. bewusst aus- und wieder ein). Nur im
 * Arbeitsspeicher: kein `localStorage`/`sessionStorage`, kein Backend — ein
 * Neustart der App vergisst "Später" wieder (ausdrücklich gewollt, §2
 * Nicht-Ziele).
 */
const snoozedServerIds = new Set<string>();

/** Nur für Tests: leert die Merkmenge, damit ein „Später“ aus einem
 * früheren Test nicht in den nächsten durchsickert (Modul-Variablen leben
 * über `beforeEach`/`render`-Aufrufe hinweg fort). */
export function resetSnoozedNoteShrinkServersForTests(): void {
  snoozedServerIds.clear();
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
  const { t } = useTranslation();
  const [suggestions, setSuggestions] = useState<PendingShrinkSuggestion[]>([]);
  const [failures, setFailures] = useState<ShrinkFailure[]>([]);

  useEffect(() => {
    const unlistenSuggested = onNoteShrinkSuggested((event) => {
      // Spec 0079, A2: für "Später" vorgemerkte Server bekommen bis zum
      // Neustart der App keine Karte mehr — früher Ausstieg, bevor
      // überhaupt State geändert wird.
      if (snoozedServerIds.has(event.serverId)) return;
      setSuggestions((prev) => [
        // spec-reviewer-Fund (Review dieses Schritts): derselbe Server kann
        // über mehrere Verbindungsenden hinweg erneut vorgeschlagen werden
        // (z. B. wenn die vorherige Karte nie beantwortet wurde) — ohne
        // Deduplizierung hätten zwei Karten denselben `key={serverId}`
        // (React-Duplicate-Key-Warnung), und ein `dismiss` hätte beide statt
        // nur einer entfernt.
        ...prev.filter((s) => s.serverId !== event.serverId),
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

  // Spec 0079, A2: merkt den Server dauerhaft (bis zum App-Neustart) vor,
  // zusätzlich zum bloßen Wegklicken der aktuellen Karte.
  const later = (serverId: string) => {
    snoozedServerIds.add(serverId);
    dismiss(serverId);
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
          className="rounded-lg border border-border bg-surface-raised p-3 text-sm shadow-lg"
        >
          <div className="flex items-start justify-between gap-2">
            <p className="font-semibold text-fg">
              {t("noteShrink.title", { name: suggestion.serverName })}
            </p>
            {/* Spec 0079, A1: gestaltet wie der ✕-Knopf der Fehler-Karten
             * unten (`dismissFailure`) — entfernt nur diese Karte, merkt den
             * Server NICHT vor (anders als „Später“). */}
            <Button
              size="sm"
              onClick={() => dismiss(suggestion.serverId)}
              aria-label={t("noteShrink.dismissHint")}
              className="shrink-0"
            >
              ✕
            </Button>
          </div>
          <p className="mt-1 text-xs text-fg-subtle">
            {t("noteShrink.body")}
          </p>
          {/* Spec 0079, A2: dritter Knopf ("Später") zusätzlich zu den
           * bisherigen zweien — bei `w-80` (320px, abzüglich `p-3` 296px
           * nutzbar) reichen drei `text-xs`/`px-3`-Knöpfe in einer Zeile
           * rechnerisch nicht mehr aus. `flex-wrap` lässt sie sauber
           * umbrechen statt sich zu stauchen. */}
          <div className="mt-2 flex flex-wrap gap-2">
            <Button size="sm" onClick={() => later(suggestion.serverId)}>
              {t("noteShrink.later")}
            </Button>
            <Button size="sm" onClick={() => editMyself(suggestion)}>
              {t("noteShrink.editMyself")}
            </Button>
            <Button variant="primary" size="sm" onClick={() => summarize(suggestion)}>
              {t("noteShrink.summarize")}
            </Button>
          </div>
        </div>
      ))}
      {failures.map((failure) => (
        <div
          key={failure.id}
          className="flex items-start justify-between gap-2 rounded-lg border border-danger-subtle-hover bg-danger-subtle p-3 text-sm shadow-lg"
        >
          <p role="alert" className="min-w-0 break-words text-danger-fg-softer">
            {failure.message}
          </p>
          <Button
            variant="danger"
            size="sm"
            onClick={() => dismissFailure(failure.id)}
            aria-label={t("noteShrink.dismissError")}
            className="shrink-0"
          >
            ✕
          </Button>
        </div>
      ))}
    </div>
  );
}
