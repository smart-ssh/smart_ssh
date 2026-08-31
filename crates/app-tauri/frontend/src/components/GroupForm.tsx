import { type FormEvent, useEffect, useMemo, useState } from "react";
import { commandErrorMessage, createGroup, deleteGroup, updateGroup } from "../api";
import type { DeleteGroupResult, GroupDto } from "../types";
import { NotesPanel } from "./NotesPanel";

interface GroupFormProps {
  /** `null` = Neuanlage. */
  groupId: string | null;
  /** Nur bei Neuanlage relevant (z. B. "+ Gruppe" innerhalb einer Gruppe). */
  defaultParentId: string | null;
  allGroups: GroupDto[];
  onSaved: () => void;
  onDeleted: () => void;
}

/** Spec 0008, Abschnitt 6: Name, Parent-Dropdown (schließt sich selbst und
 * eigene Nachfahren clientseitig aus), Notiz-Editor, Löschen mit
 * Cascade-Vorschau. */
export function GroupForm({ groupId, defaultParentId, allGroups, onSaved, onDeleted }: GroupFormProps) {
  const isCreate = groupId === null;
  const existing = useMemo(() => allGroups.find((g) => g.id === groupId) ?? null, [allGroups, groupId]);

  const [name, setName] = useState(existing?.name ?? "");
  const [parentId, setParentId] = useState<string | null>(existing?.parentId ?? defaultParentId);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deletePreview, setDeletePreview] = useState<DeleteGroupResult | null>(null);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    setName(existing?.name ?? "");
    setParentId(existing?.parentId ?? defaultParentId);
    setDeletePreview(null);
    setError(null);
  }, [groupId, existing, defaultParentId]);

  // Spec 0008, Abschnitt 6: "schließt die Gruppe selbst und ihre
  // Nachfahren clientseitig aus der Auswahl aus".
  const excludedIds = useMemo(() => {
    if (!groupId) return new Set<string>();
    const excluded = new Set<string>([groupId]);
    let changed = true;
    while (changed) {
      changed = false;
      for (const g of allGroups) {
        if (g.parentId && excluded.has(g.parentId) && !excluded.has(g.id)) {
          excluded.add(g.id);
          changed = true;
        }
      }
    }
    return excluded;
  }, [groupId, allGroups]);

  const availableParents = allGroups.filter((g) => !excludedIds.has(g.id));

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setError(null);
    try {
      if (isCreate) {
        await createGroup(name, parentId);
      } else if (groupId) {
        await updateGroup(groupId, name, parentId);
      }
      onSaved();
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const handleDeleteClick = async () => {
    if (!groupId) return;
    setError(null);
    try {
      setDeletePreview(await deleteGroup(groupId, false));
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  const handleConfirmDelete = async () => {
    if (!groupId) return;
    setDeleting(true);
    setError(null);
    try {
      await deleteGroup(groupId, true);
      onDeleted();
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="max-w-xl space-y-6 p-4">
      <h2 className="text-lg font-semibold text-slate-100">
        {isCreate ? "Neue Gruppe" : `Gruppe: ${existing?.name ?? ""}`}
      </h2>

      <form onSubmit={handleSubmit} className="space-y-3">
        <label className="block text-sm text-slate-300">
          Name
          <input
            type="text"
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          />
        </label>

        <label className="block text-sm text-slate-300">
          Übergeordnete Gruppe
          <select
            value={parentId ?? ""}
            onChange={(e) => setParentId(e.target.value || null)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="">(keine)</option>
            {availableParents.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </select>
        </label>

        {error && <p className="text-sm text-red-400">{error}</p>}

        <button
          type="submit"
          disabled={saving}
          className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
        >
          {saving ? "Speichert…" : isCreate ? "Anlegen" : "Speichern"}
        </button>
      </form>

      {!isCreate && groupId && existing && (
        <>
          <NotesPanel target={{ Group: groupId }} currentNotes={existing.notes} onNotesChanged={onSaved} />

          <div className="border-t border-slate-700 pt-4">
            <button
              type="button"
              onClick={handleDeleteClick}
              className="rounded bg-red-900 px-3 py-1.5 text-sm text-red-200 hover:bg-red-800"
            >
              Gruppe löschen
            </button>

            {deletePreview && (
              <div className="mt-3 rounded border border-red-800 bg-red-950 p-3 text-sm">
                <p className="mb-2 font-medium text-red-200">Auswirkungen des Löschens:</p>
                {deletePreview.childGroupsToDelete.length === 0 &&
                deletePreview.serversToUnassign.length === 0 ? (
                  <p className="text-red-200">Keine weiteren Objekte betroffen.</p>
                ) : (
                  <ul className="mb-2 space-y-1 text-red-200">
                    {deletePreview.childGroupsToDelete.map((g) => (
                      <li key={g.id}>Untergruppe wird mitgelöscht: {g.name}</li>
                    ))}
                    {deletePreview.serversToUnassign.map((s) => (
                      <li key={s.id}>Server wird nur entkoppelt (bleibt erhalten): {s.name}</li>
                    ))}
                  </ul>
                )}
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => setDeletePreview(null)}
                    className="rounded bg-slate-700 px-3 py-1 text-xs hover:bg-slate-600"
                  >
                    Abbrechen
                  </button>
                  <button
                    type="button"
                    onClick={handleConfirmDelete}
                    disabled={deleting}
                    className="rounded bg-red-700 px-3 py-1 text-xs text-white hover:bg-red-600 disabled:opacity-50"
                  >
                    {deleting ? "Löscht…" : "Endgültig löschen"}
                  </button>
                </div>
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
