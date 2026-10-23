import type { TFunction } from "i18next";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode, commandErrorMessage, respondToAction } from "../api";
import { onNoteUpdateExpired, onNoteUpdateSuggested } from "../events";
import type { NoteUpdateSuggestedEvent } from "../types";
import { NoteDiffPreview } from "./NoteDiffPreview";
import { Button } from "./ui";

interface PendingSuggestion {
  sessionId: string;
  actionId: string;
  /** Spec 0016, Abschnitt 6: nur noch relativ zur Session ("dieser
   * Server"/"dessen Gruppe"), nie eine konkrete ID — die KI kennt/liefert
   * keine, das Backend löst sie selbst auf `session.server_id` auf. */
  targetKind: "server" | "group";
  /** Spec 0023, Abschnitt 3: Server- oder Gruppenname des Ziels, immer
   * prominent anzuzeigen — `null` nur, wenn die Zielauflösung serverseitig
   * fehlschlug (z. B. Server inzwischen gelöscht). */
  targetName: string | null;
  newContent: string;
  /** Spec 0019, Abschnitt 3/4: aktueller Inhalt des Ziels, für die
   * Diff-Vorschau statt des vollen neuen Texts. */
  previousNoteContent: string | null;
  /** Spec 0057, §4.2 (Issue #94): die Kürzung wurde vom Provider
   * abgeschnitten — die App zeigt dann eine eigene Warnung über dem Diff,
   * außerhalb des KI-generierten Inhalts. */
  summaryIncomplete: boolean;
  /** Kompakte Ansicht per Default (Spec 0010, Abschnitt 2, Punkt 6: "dezente
   * Benachrichtigung statt eines blockierenden Modals") — erst nach Klick
   * auf "Anzeigen" wird der Inhalt (das "Dialog"-Äquivalent) eingeblendet. */
  expanded: boolean;
  deciding: boolean;
  /** Issue #628: the backend no longer waits for an answer (timeout or
   * unknown id) — the card only says so and can be closed. */
  expired: boolean;
  error: string | null;
}

function targetKindFromEvent(event: NoteUpdateSuggestedEvent): "server" | "group" {
  return event.action.ProposeNoteUpdate.target === "CurrentServer" ? "server" : "group";
}

/** Spec 0023, Abschnitt 3/4: prominenter Titel statt der früheren
 * "dieser Server"/"dessen Gruppe"-Formulierung, die stillschweigend
 * annahm, der Nutzer habe gerade den betroffenen Server offen — genau die
 * Annahme, die den gemeldeten Bug verursacht hat. Gruppen-Vorschläge
 * bekommen bewusst das eigene Wort "Gruppen-Notiz-Vorschlag" (nicht
 * "Server-Notiz-Vorschlag für 'X'"), um die Verwechslungsgefahr auf der
 * Server-vs-Gruppe-Achse zu vermeiden. */
function suggestionTitle(t: TFunction, kind: "server" | "group", targetName: string | null): string {
  const name = targetName ?? t("noteSuggestion.unknownTarget");
  return kind === "group"
    ? t("noteSuggestion.groupTitle", { name })
    : t("noteSuggestion.serverTitle", { name });
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
  const { t } = useTranslation();
  const [suggestions, setSuggestions] = useState<PendingSuggestion[]>([]);

  useEffect(() => {
    const unlisten = onNoteUpdateSuggested((event) => {
      setSuggestions((prev) => [
        ...prev,
        {
          sessionId: event.sessionId,
          actionId: event.actionId,
          targetKind: targetKindFromEvent(event),
          targetName: event.targetName,
          newContent: event.action.ProposeNoteUpdate.new_content,
          previousNoteContent: event.previousNoteContent,
          summaryIncomplete: event.summaryIncomplete === true,
          expanded: false,
          deciding: false,
          expired: false,
          error: null,
        },
      ]);
    });
    const unlistenExpired = onNoteUpdateExpired((event) => {
      setSuggestions((prev) =>
        prev.map((s) =>
          s.actionId === event.actionId
            ? { ...s, expired: true, deciding: false, error: null }
            : s,
        ),
      );
    });
    return () => {
      unlisten.then((fn) => fn());
      unlistenExpired.then((fn) => fn());
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
      if (commandErrorCode(err) === "ACTION_CONFIRMATION_UNKNOWN") {
        updateSuggestion(suggestion.actionId, { expired: true, deciding: false, error: null });
        return;
      }
      updateSuggestion(suggestion.actionId, {
        deciding: false,
        error: commandErrorMessage(err),
      });
    }
  };

  /** ✕: an open suggestion counts as Reject; an expired one is only hidden. */
  const close = (suggestion: PendingSuggestion) => {
    if (suggestion.expired) dismiss(suggestion.actionId);
    else void decide(suggestion, "deny");
  };

  if (suggestions.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2">
      {suggestions.map((suggestion) => (
        <div
          key={suggestion.actionId}
          className="rounded-lg border border-slate-700 bg-slate-800 p-3 text-sm shadow-lg"
        >
          {suggestion.expired ? (
            <div className="flex items-center justify-between gap-2">
              <div className="space-y-1">
                <p className="font-semibold text-slate-100">
                  {suggestionTitle(t, suggestion.targetKind, suggestion.targetName)}
                </p>
                <output className="block text-xs text-slate-300">
                  {t("noteSuggestion.expired")}
                </output>
              </div>
              <Button
                size="sm"
                onClick={() => close(suggestion)}
                aria-label={t("noteSuggestion.dismiss")}
              >
                ✕
              </Button>
            </div>
          ) : !suggestion.expanded ? (
            <div className="flex items-center justify-between gap-2">
              <span className="font-semibold text-slate-100">
                {suggestionTitle(t, suggestion.targetKind, suggestion.targetName)}
              </span>
              <div className="flex shrink-0 gap-1">
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => updateSuggestion(suggestion.actionId, { expanded: true })}
                >
                  {t("noteSuggestion.show")}
                </Button>
                <Button
                  size="sm"
                  onClick={() => close(suggestion)}
                  disabled={suggestion.deciding}
                  aria-label={t("noteSuggestion.dismiss")}
                >
                  ✕
                </Button>
              </div>
            </div>
          ) : (
            <div className="space-y-2">
              <div className="flex items-start justify-between gap-2">
                <p className="font-semibold text-slate-100">
                  {suggestionTitle(t, suggestion.targetKind, suggestion.targetName)}
                </p>
                <Button
                  size="sm"
                  onClick={() => close(suggestion)}
                  disabled={suggestion.deciding}
                  aria-label={t("noteSuggestion.dismiss")}
                >
                  ✕
                </Button>
              </div>
              {suggestion.summaryIncomplete && (
                <p
                  role="alert"
                  className="rounded border border-amber-600 bg-amber-950 px-2 py-1 text-xs text-amber-200"
                >
                  {t("noteSuggestion.summaryIncomplete")}
                </p>
              )}
              <div className="max-h-40 overflow-y-auto">
                <NoteDiffPreview
                  previousContent={suggestion.previousNoteContent}
                  newContent={suggestion.newContent}
                />
              </div>
              {suggestion.error && (
                <p role="alert" className="text-xs text-red-300">
                  {suggestion.error}
                </p>
              )}
              <div className="flex gap-2">
                <Button
                  variant="danger"
                  size="sm"
                  onClick={() => decide(suggestion, "deny")}
                  disabled={suggestion.deciding}
                >
                  {t("noteSuggestion.reject")}
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => decide(suggestion, "approve")}
                  disabled={suggestion.deciding}
                >
                  {suggestion.deciding ? t("noteSuggestion.applying") : t("noteSuggestion.apply")}
                </Button>
              </div>
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
