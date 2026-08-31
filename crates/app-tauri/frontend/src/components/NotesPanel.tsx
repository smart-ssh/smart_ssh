import { useEffect, useState } from "react";
import {
  commandErrorMessage,
  listNoteRevisions,
  rollbackNote,
  updateGroupNotes,
  updateServerNotes,
} from "../api";
import type { NoteRevisionDto, NoteTarget } from "../types";

interface NotesPanelProps {
  target: NoteTarget;
  currentNotes: string;
  /** Nach erfolgreichem Speichern/Rollback — Elternformular lädt den
   * Server/die Gruppe neu, damit `currentNotes` aktuell bleibt. */
  onNotesChanged: () => void;
}

/**
 * Notiz-Editor + Historie (Spec 0008, Abschnitt 5/6) — gemeinsam für
 * Gruppen- und Server-Formular, da beide identisch funktionieren (nur das
 * `NoteTarget` unterscheidet sich).
 */
export function NotesPanel({ target, currentNotes, onNotesChanged }: NotesPanelProps) {
  const [draft, setDraft] = useState(currentNotes);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showHistory, setShowHistory] = useState(false);
  const [revisions, setRevisions] = useState<NoteRevisionDto[]>([]);

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

  return (
    <div className="space-y-2">
      <label className="block text-sm text-slate-300">
        Notiz (Kontext für die KI)
        <textarea
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
          {revisions.map((r) => (
            <li key={r.id} className="px-3 py-2 text-sm">
              <div className="mb-1 flex items-center justify-between">
                <span className="text-xs text-slate-400">
                  {new Date(r.createdAt).toLocaleString()} ·{" "}
                  {r.editedBy.kind === "user"
                    ? "Nutzer"
                    : `KI (${r.editedBy.provider}/${r.editedBy.model})`}
                </span>
                <button
                  type="button"
                  onClick={() => handleRollback(r.id)}
                  className="rounded bg-slate-700 px-2 py-0.5 text-xs hover:bg-slate-600"
                >
                  Wiederherstellen
                </button>
              </div>
              <p className="whitespace-pre-wrap text-slate-300">{r.content}</p>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
