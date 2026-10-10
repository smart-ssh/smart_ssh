import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode, commandErrorMessage, createGroup, deleteGroup, updateGroup } from "../api";
import { translateErrorCode } from "../errorCodes";
import { flattenGroupOptions, groupOptionLabel } from "../groupTree";
import type { DeleteGroupResult, GroupDto } from "../types";
import { NotesPanel } from "./NotesPanel";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";

/** Issue #63: neuer Ort des geöffneten Elements nach einem erfolgreichen
 * Verschieben per Drag-and-drop — `groupId` ist die neue Gruppe (Server)
 * bzw. Übergruppe (Gruppe), `null` = Wurzelebene. Ein neues Objekt je
 * Verschieben; das Formular übernimmt daraus nur dieses eine Feld. */
export interface MovedTo {
  groupId: string | null;
}

interface GroupFormProps {
  /** `null` = Neuanlage. */
  groupId: string | null;
  /** Nur bei Neuanlage relevant (z. B. "+ Gruppe" innerhalb einer Gruppe). */
  defaultParentId: string | null;
  allGroups: GroupDto[];
  onSaved: () => void;
  onDeleted: () => void;
  /** Issue #63: s. [`MovedTo`]. `null`/fehlend = nicht verschoben. */
  movedTo?: MovedTo | null;
}

/** Spec 0008, Abschnitt 6: Name, Parent-Dropdown (schließt sich selbst und
 * eigene Nachfahren clientseitig aus), Notiz-Editor, Löschen mit
 * Cascade-Vorschau. */
export function GroupForm({
  groupId,
  defaultParentId,
  allGroups,
  onSaved,
  onDeleted,
  movedTo = null,
}: GroupFormProps) {
  const { t } = useTranslation();
  const isCreate = groupId === null;
  const existing = useMemo(() => allGroups.find((g) => g.id === groupId) ?? null, [allGroups, groupId]);

  const [name, setName] = useState(existing?.name ?? "");
  const [parentId, setParentId] = useState<string | null>(existing?.parentId ?? defaultParentId);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deletePreview, setDeletePreview] = useState<DeleteGroupResult | null>(null);
  const [deleting, setDeleting] = useState(false);

  // Issue #63: Felder nur dann aus `existing`/`defaultParentId` füllen,
  // wenn sich das bearbeitete Element selbst ändert (oder `existing` zum
  // ersten Mal verfügbar wird) — NICHT bei jedem Neuladen von `allGroups`.
  // `existing` ist nach jedem `reload()` ein neues Objekt; früher setzte
  // das jede ungespeicherte Eingabe (z. B. den Namen) zurück, etwa nach
  // dem Verschieben einer beliebigen Gruppe oder eines Servers.
  const syncedFor = useRef<string | null>(null);
  useEffect(() => {
    const identity = isCreate ? `new:${defaultParentId ?? ""}` : existing ? `group:${existing.id}` : null;
    if (identity === null || identity === syncedFor.current) return;
    syncedFor.current = identity;
    setName(existing?.name ?? "");
    setParentId(existing?.parentId ?? defaultParentId);
    setDeletePreview(null);
    setError(null);
  }, [isCreate, existing, defaultParentId]);

  // Issue #63: nach einem erfolgreichen Verschieben dieser Gruppe nur die
  // Übergruppe übernehmen — der Name und alles andere bleiben, wie der
  // Nutzer sie gerade eingetippt hat.
  useEffect(() => {
    if (movedTo) setParentId(movedTo.groupId);
  }, [movedTo]);

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

  // Issue #49: hierarchisch (eingerückter voller Pfad) statt flach.
  const availableParents = flattenGroupOptions(allGroups).filter(
    (option) => !excludedIds.has(option.group.id),
  );

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
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
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
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
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
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="max-w-xl space-y-6 p-4">
      <h2 className="font-heading text-lg font-semibold tracking-wide text-slate-100">
        {isCreate ? t("groupForm.titleNew") : t("groupForm.titleExisting", { name: existing?.name ?? "" })}
      </h2>

      <form onSubmit={handleSubmit} className="space-y-3">
        <label className="block text-sm text-slate-300">
          {t("common.name")}
          <input
            {...TECHNICAL_INPUT_PROPS}
            type="text"
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          />
        </label>

        <label className="block text-sm text-slate-300">
          {t("groupForm.parent")}
          <select
            value={parentId ?? ""}
            onChange={(e) => setParentId(e.target.value || null)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="">{t("groupForm.noParent")}</option>
            {availableParents.map((option) => (
              <option key={option.group.id} value={option.group.id}>
                {groupOptionLabel(option)}
              </option>
            ))}
          </select>
        </label>

        {error && <p className="text-sm text-red-400">{error}</p>}

        <button
          type="submit"
          disabled={saving}
          className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-slate-950 hover:bg-indigo-500 disabled:opacity-50"
        >
          {saving ? t("common.saving") : isCreate ? t("common.create") : t("common.save")}
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
              {t("groupForm.delete")}
            </button>

            {deletePreview && (
              <div className="mt-3 rounded border border-red-800 bg-red-950 p-3 text-sm">
                <p className="mb-2 font-medium text-red-200">{t("groupForm.deleteImpactTitle")}</p>
                {deletePreview.childGroupsToDelete.length === 0 &&
                deletePreview.serversToUnassign.length === 0 &&
                deletePreview.unusableServersToUnassign.length === 0 ? (
                  <p className="text-red-200">{t("groupForm.deleteNoImpact")}</p>
                ) : (
                  <ul className="mb-2 space-y-1 text-red-200">
                    {deletePreview.childGroupsToDelete.map((g) => (
                      <li key={g.id}>{t("groupForm.childGroupWillBeDeleted", { name: g.name })}</li>
                    ))}
                    {deletePreview.serversToUnassign.map((s) => (
                      <li key={s.id}>{t("groupForm.serverWillBeUnassigned", { name: s.name })}</li>
                    ))}
                    {deletePreview.unusableServersToUnassign.map((s) => (
                      <li key={s.id}>
                        {t("groupForm.unusableServerWillBeUnassigned", { name: s.name })}
                      </li>
                    ))}
                  </ul>
                )}
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => setDeletePreview(null)}
                    className="rounded bg-slate-700 px-3 py-1 text-xs hover:bg-slate-600"
                  >
                    {t("common.cancel")}
                  </button>
                  <button
                    type="button"
                    onClick={handleConfirmDelete}
                    disabled={deleting}
                    className="rounded bg-red-700 px-3 py-1 text-xs text-white hover:bg-red-600 disabled:opacity-50"
                  >
                    {deleting ? t("common.deleting") : t("groupForm.confirmDelete")}
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
