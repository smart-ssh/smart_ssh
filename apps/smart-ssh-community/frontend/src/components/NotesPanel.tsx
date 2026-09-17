import { useEffect, useRef, useState } from "react";
import {
  commandErrorMessage,
  largeNoteDialogThresholdBytes,
  listNoteRevisions,
  requestNoteShrink,
  rollbackNote,
  updateGroupNotes,
  updateServerNotes,
} from "../api";
import type { NoteRevisionDto, NoteTarget } from "../types";
import { NoteDiffPreview } from "./NoteDiffPreview";

/** UTF-8-Byte-Länge statt `string.length` (UTF-16-Code-Einheiten) — der
 * Backend-Schwellwert (`orchestration::LARGE_NOTE_DIALOG_THRESHOLD_BYTES`)
 * zählt Byte, exakt wie `server.notes.len()` in Rust. Bei mehrbyte-Zeichen
 * (Umlaute, Emoji) würden beide Zählweisen sonst auseinanderlaufen. */
function utf8ByteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

interface NotesPanelProps {
  target: NoteTarget;
  currentNotes: string;
  /** Nach erfolgreichem Speichern/Rollback — Elternformular lädt den
   * Server/die Gruppe neu, damit `currentNotes` aktuell bleibt. */
  onNotesChanged: () => void;
  /** Spec 0058, Teil 2 (Etappe-4-Review-Fund): "Mache ich selbst" im
   * Kürzungs-Vorschlags-Dialog öffnete bisher nur das Server-Formular, ohne
   * zum Notizfeld zu scrollen/es zu fokussieren — der Nutzer musste es
   * erst suchen. `true` genau dann, wenn dieses Formular DESHALB geöffnet
   * wurde (einmalig beim Mounten ausgewertet, s. Effekt unten); Default
   * `false` für den regulären Aufruf über die Sidebar-Navigation. */
  autoFocus?: boolean;
}

/**
 * Notiz-Editor + Historie (Spec 0008, Abschnitt 5/6) — gemeinsam für
 * Gruppen- und Server-Formular, da beide identisch funktionieren (nur das
 * `NoteTarget` unterscheidet sich).
 */
export function NotesPanel({ target, currentNotes, onNotesChanged, autoFocus = false }: NotesPanelProps) {
  const [draft, setDraft] = useState(currentNotes);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showHistory, setShowHistory] = useState(false);
  const [revisions, setRevisions] = useState<NoteRevisionDto[]>([]);
  // Spec 0030, Abschnitt 3: pro Eintrag einzeln auf-/zuklappbar, mehrere
  // gleichzeitig möglich — standardmäßig leer (alles eingeklappt).
  const [expandedRevisionIds, setExpandedRevisionIds] = useState<Set<string>>(new Set());
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  // Spec 0058, Teil 1 (Etappe 5): `null`, solange der Schwellwert noch
  // nicht geladen ist — der Hinweis bleibt in dieser Zeit ausgeblendet
  // (kein falsches "nicht groß genug"-Aufblitzen).
  const [largeNoteThreshold, setLargeNoteThreshold] = useState<number | null>(null);
  const [shrinkRequesting, setShrinkRequesting] = useState(false);
  const [shrinkError, setShrinkError] = useState<string | null>(null);

  useEffect(() => {
    largeNoteDialogThresholdBytes()
      .then(setLargeNoteThreshold)
      .catch((err) => console.error(commandErrorMessage(err)));
  }, []);

  // Nur beim Mounten ausgewertet (leere Dependency-Liste) — `NotesPanel`
  // wird über `ServerForm`s `key={selection.id}` (`ManagementView.tsx`)
  // bei jedem Server-/Gruppenwechsel frisch gemountet, ein einmaliges
  // Scroll+Fokus beim Öffnen ist also korrekt, kein erneutes Feuern bei
  // jedem Tippen im Feld nötig.
  useEffect(() => {
    if (autoFocus) {
      textareaRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
      textareaRef.current?.focus();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const toggleExpanded = (revisionId: string) => {
    setExpandedRevisionIds((prev) => {
      const next = new Set(prev);
      if (next.has(revisionId)) {
        next.delete(revisionId);
      } else {
        next.add(revisionId);
      }
      return next;
    });
  };

  useEffect(() => setDraft(currentNotes), [currentNotes]);

  const loadHistory = () => {
    listNoteRevisions(target)
      .then(setRevisions)
      .catch((err) => setError(commandErrorMessage(err)));
  };

  useEffect(() => {
    if (showHistory) loadHistory();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showHistory, target]);

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    try {
      if ("Server" in target) {
        await updateServerNotes(target.Server, draft);
      } else {
        await updateGroupNotes(target.Group, draft);
      }
      onNotesChanged();
      if (showHistory) loadHistory();
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const handleRollback = async (revisionId: string) => {
    setError(null);
    try {
      await rollbackNote(target, revisionId);
      onNotesChanged();
      loadHistory();
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  // Spec 0058, Teil 1 (Etappe 5), optionaler "jetzt zusammenfassen"-Link:
  // löst denselben KI-Kürzungs-Fluss wie der Sitzungsende-Dialog aus
  // (Etappe 4, `commands::request_note_shrink`) — inkl. Diff-Bestätigung
  // über die bereits an der App-Wurzel gemountete `NoteSuggestionToast`.
  // Nur für Server-Notizen: `NoteTargetSelector::CurrentServer` kennt keine
  // Gruppen-Variante (s. `orchestration::execute_note_shrink_request`).
  const handleSummarizeNow = async () => {
    if (!("Server" in target)) return;
    setShrinkRequesting(true);
    setShrinkError(null);
    try {
      await requestNoteShrink(target.Server);
    } catch (err) {
      setShrinkError(commandErrorMessage(err));
    } finally {
      setShrinkRequesting(false);
    }
  };

  const isLarge = largeNoteThreshold !== null && utf8ByteLength(draft) >= largeNoteThreshold;

  return (
    <div className="space-y-2">
      {isLarge && (
        <div className="rounded border border-amber-800/50 bg-amber-950/30 px-3 py-2 text-xs text-amber-300">
          <p>
            Diese Notiz ist sehr groß und kann bei langen Sitzungen für den KI-Kontext gekürzt
            werden. Die gespeicherte Notiz bleibt vollständig erhalten.
          </p>
          {"Server" in target && (
            <>
              <button
                type="button"
                onClick={handleSummarizeNow}
                disabled={shrinkRequesting}
                className="mt-1.5 underline hover:no-underline disabled:opacity-50"
              >
                {shrinkRequesting ? "Wird angefragt…" : "Jetzt zusammenfassen"}
              </button>
              {shrinkError && <p className="mt-1 text-red-400">{shrinkError}</p>}
            </>
          )}
        </div>
      )}

      <label className="block text-sm text-slate-300">
        Notiz (Kontext für die KI)
        <textarea
          ref={textareaRef}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          rows={6}
          className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-sm text-slate-100"
        />
      </label>

      {error && <p className="text-sm text-red-400">{error}</p>}

      <div className="flex gap-2">
        <button
          type="button"
          onClick={handleSave}
          disabled={saving || draft === currentNotes}
          className="rounded bg-indigo-600 px-3 py-1.5 text-sm text-white hover:bg-indigo-500 disabled:opacity-50"
        >
          {saving ? "Speichert…" : "Notiz speichern"}
        </button>
        <button
          type="button"
          onClick={() => setShowHistory((s) => !s)}
          className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700"
        >
          {showHistory ? "Historie ausblenden" : "Historie anzeigen"}
        </button>
      </div>

      {showHistory && (
        <ul className="divide-y divide-slate-700 rounded border border-slate-700">
          {revisions.length === 0 && (
            <li className="px-3 py-2 text-sm text-slate-400">Noch keine Historie.</li>
          )}
          {revisions.map((r, index) => {
            // Spec 0030, Abschnitt 3: `revisions` kommt chronologisch
            // aufsteigend vom Backend (`ORDER BY created_at`) — der direkte
            // Vorgänger einer Revision ist also der vorherige Array-Index,
            // nicht der jeweils erste/letzte Eintrag der Liste. Index 0 hat
            // keinen Vorgänger ("Ursprüngliche Version").
            const previous = index > 0 ? revisions[index - 1] : null;
            const expanded = expandedRevisionIds.has(r.id);
            return (
              <li key={r.id} className="text-sm">
                <div className="flex items-center justify-between px-3 py-2">
                  <button
                    type="button"
                    onClick={() => toggleExpanded(r.id)}
                    aria-expanded={expanded}
                    className="flex flex-1 items-center gap-1.5 text-left text-xs text-slate-400 hover:text-slate-200"
                  >
                    <span className="select-none">{expanded ? "▾" : "▸"}</span>
                    {new Date(r.createdAt).toLocaleString()} ·{" "}
                    {r.editedBy.kind === "user"
                      ? "Nutzer"
                      : `KI (${r.editedBy.provider}/${r.editedBy.model})`}
                  </button>
                  <button
                    type="button"
                    onClick={() => handleRollback(r.id)}
                    className="shrink-0 rounded bg-slate-700 px-2 py-0.5 text-xs hover:bg-slate-600"
                  >
                    Wiederherstellen
                  </button>
                </div>
                {expanded && (
                  <div className="px-3 pb-2">
                    {previous === null ? (
                      <div>
                        <p className="mb-1 text-xs font-semibold text-slate-500">
                          Ursprüngliche Version
                        </p>
                        <p className="whitespace-pre-wrap text-slate-300">{r.content}</p>
                      </div>
                    ) : (
                      <NoteDiffPreview previousContent={previous.content} newContent={r.content} />
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
