import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { applySshConfigImport, commandErrorMessage, previewSshConfigImport } from "../api";
import { showToast } from "../toastBus";
import type {
  SshConfigEntryChoice,
  SshConfigIdentityMode,
  SshConfigImportPreviewDto,
  SshConfigPreviewEntryDto,
  SshConfigPreviewTagDto,
} from "../types";

interface SshConfigImportDialogProps {
  onClose: () => void;
  /** Nach einem erfolgreichen `apply` — die Sidebar/Verwalten-Ansicht lädt
   * damit die neuen Server/Gruppen nach. */
  onImported: () => void;
}

/** Spec 0075, §5.2a/Q-BL-0216-02 (K3, noch offen — s. Frage-Datei): Ob ein
 * Schlagwort standardmäßig angewählt bleibt. **Einzige Stelle**, die sich
 * ändern muss, sollte die Antwort auf Q-BL-0216-02 eine andere Vorgabe für
 * buchstäbliche Treffer auf eine Allow-Regel verlangen (Architekten-
 * Empfehlung dort: Vorgabe nur für DIESEN Fall auf abgewählt drehen). Bis
 * dahin unverändert der Kern-Vorgabe: alles angewählt — die Kennzeichnung
 * in der Oberfläche (nicht die Vorgabe) trägt hier die Sichtbarkeit.
 */
function defaultTagSelected(_tag: SshConfigPreviewTagDto): boolean {
  return true;
}

interface EntryUiState {
  selected: boolean;
  /** `null` = folgt der globalen Wahl (§3.1.9: „für den ganzen Import und
   * einzeln je Eintrag umstellbar"). */
  identityModeOverride: SshConfigIdentityMode | null;
  renameTo: string;
  droppedTags: Set<string>;
}

function initialEntryState(entry: SshConfigPreviewEntryDto): EntryUiState {
  return {
    selected: true,
    identityModeOverride: null,
    renameTo: "",
    droppedTags: new Set(entry.tags.filter((t) => !defaultTagSelected(t)).map((t) => t.tag)),
  };
}

function groupDepth(groups: SshConfigImportPreviewDto["groups"], index: number): number {
  let depth = 0;
  let cur = groups[index]?.parent ?? null;
  while (cur !== null) {
    depth += 1;
    cur = groups[cur]?.parent ?? null;
  }
  return depth;
}

function originLabel(
  t: ReturnType<typeof useTranslation>["t"],
  origin: { file: string; line: number; block: string } | null,
): string {
  if (!origin) return t("sshConfigImport.origin.default");
  const file = origin.file.split(/[/\\]/).pop() ?? origin.file;
  return t("sshConfigImport.origin.fromFile", { file, line: origin.line, block: origin.block });
}

/** Spec 0075, §3.1.7/§5.2a — Vorschau-Dialog vor dem `ssh_config`-Import:
 * vollständige Liste der entstehenden Profile samt Herkunft der Werte,
 * gelesene Dateien, nicht übernommene Direktiven, Gruppenstruktur und
 * Konflikte. Jeder Eintrag einzeln abwählbar; Abbrechen legt nichts an. */
export function SshConfigImportDialog({ onClose, onImported }: SshConfigImportDialogProps) {
  const { t } = useTranslation();
  const [preview, setPreview] = useState<SshConfigImportPreviewDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [applying, setApplying] = useState(false);
  const [globalIdentityMode, setGlobalIdentityMode] = useState<SshConfigIdentityMode>("keepAsFile");
  const [entryState, setEntryState] = useState<Record<number, EntryUiState>>({});

  useEffect(() => {
    let cancelled = false;
    previewSshConfigImport(t("sshConfigImport.dialogTitle"))
      .then((dto) => {
        if (cancelled) return;
        if (!dto) {
          onClose();
          return;
        }
        setPreview(dto);
        const initial: Record<number, EntryUiState> = {};
        for (const e of dto.entries) initial[e.index] = initialEntryState(e);
        setEntryState(initial);
      })
      .catch((err) => !cancelled && setError(commandErrorMessage(err)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const effectiveMode = (index: number): SshConfigIdentityMode =>
    entryState[index]?.identityModeOverride ?? globalIdentityMode;

  // §3.1.9 (b): "Die Vorschau nennt jede Datei, die dabei geöffnet würde…
  // vorher" — neu berechnet bei jeder Änderung der Wahl, nicht die
  // statische Liste aller `IdentityFile`-Pfade aus dem DTO.
  const filesOpenedOnConfirm = useMemo(() => {
    if (!preview) return [];
    const paths = new Set<string>();
    for (const e of preview.entries) {
      const st = entryState[e.index];
      if (!st?.selected || !e.identityFile) continue;
      if (effectiveMode(e.index) === "intoKeychain") paths.add(e.identityFile.path);
    }
    return [...paths].sort();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [preview, entryState, globalIdentityMode]);

  const setEntry = (index: number, patch: Partial<EntryUiState>) =>
    setEntryState((prev) => ({ ...prev, [index]: { ...prev[index], ...patch } }));

  const toggleTag = (index: number, tag: string) =>
    setEntryState((prev) => {
      const cur = prev[index];
      const dropped = new Set(cur.droppedTags);
      if (dropped.has(tag)) dropped.delete(tag);
      else dropped.add(tag);
      return { ...prev, [index]: { ...cur, droppedTags: dropped } };
    });

  const handleConfirm = async () => {
    if (!preview) return;
    setApplying(true);
    setError(null);
    const choices: SshConfigEntryChoice[] = preview.entries.map((e) => {
      const st = entryState[e.index];
      return {
        index: e.index,
        selected: st.selected,
        identityMode: effectiveMode(e.index),
        renameTo: e.conflict && st.renameTo.trim() ? st.renameTo.trim() : null,
        droppedTags: [...st.droppedTags],
      };
    });
    try {
      const outcome = await applySshConfigImport(choices);
      const fallbackCount = outcome.identityFallbacks.length;
      showToast({
        kind: "success",
        message:
          fallbackCount > 0
            ? t("sshConfigImport.result.withFallbacks", {
                created: outcome.createdServers,
                groups: outcome.createdGroups,
                skipped: outcome.skippedConflicts,
                fallbacks: fallbackCount,
              })
            : t("sshConfigImport.result.success", {
                created: outcome.createdServers,
                groups: outcome.createdGroups,
                skipped: outcome.skippedConflicts,
              }),
      });
      onImported();
      onClose();
    } catch (err) {
      setError(commandErrorMessage(err));
      setApplying(false);
    }
  };

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <div className="flex max-h-[90vh] w-full max-w-4xl flex-col overflow-hidden rounded border border-slate-600 bg-slate-900 shadow-xl">
        <div className="border-b border-slate-700 p-4">
          <h2 className="font-heading text-lg font-semibold text-slate-100">
            {t("sshConfigImport.dialogTitle")}
          </h2>
        </div>

        <div className="flex-1 overflow-y-auto p-4 text-sm text-slate-300">
          {loading && <p>{t("common.loading")}</p>}
          {error && <p className="mb-3 rounded border border-red-700 bg-red-950/50 p-2 text-red-300">{error}</p>}

          {preview && (
            <>
              {/* Gelesene Dateien (§3.1.4) */}
              <section className="mb-4">
                <h3 className="mb-1 font-semibold text-slate-100">{t("sshConfigImport.filesHeading")}</h3>
                <ul className="space-y-0.5 font-mono text-xs">
                  {preview.files.map((f) => (
                    <li key={f.path} className="flex gap-2">
                      <span className="text-slate-500">{"  ".repeat(f.depth)}</span>
                      <span className="break-all">{f.path}</span>
                      <span className="text-slate-500">— {t(`sshConfigImport.fileStatus.${f.status}`)}</span>
                    </li>
                  ))}
                </ul>
              </section>

              {/* Gruppenstruktur (§4.5) */}
              {preview.groups.length > 0 && (
                <section className="mb-4">
                  <h3 className="mb-1 font-semibold text-slate-100">{t("sshConfigImport.groupsHeading")}</h3>
                  <ul className="space-y-0.5 text-xs">
                    {preview.groups.map((g, i) => (
                      <li key={`${g.sourcePath}-${g.name}`} style={{ paddingLeft: `${groupDepth(preview.groups, i) * 14}px` }}>
                        📁 {g.name}
                      </li>
                    ))}
                  </ul>
                </section>
              )}

              {/* Weg (b): globale Wahl + Dateien, die dabei aufgingen (§3.1.9) */}
              <section className="mb-4 rounded border border-slate-700 p-3">
                <h3 className="mb-2 font-semibold text-slate-100">{t("sshConfigImport.identityFile.modeHeading")}</h3>
                <div className="mb-2 flex gap-4">
                  {(["keepAsFile", "intoKeychain", "drop"] as const).map((mode) => (
                    <label key={mode} className="flex items-center gap-1">
                      <input
                        type="radio"
                        name="global-identity-mode"
                        checked={globalIdentityMode === mode}
                        onChange={() => setGlobalIdentityMode(mode)}
                      />
                      {t(`sshConfigImport.identityFile.mode.${mode}`)}
                    </label>
                  ))}
                </div>
                {filesOpenedOnConfirm.length > 0 && (
                  <div className="rounded border border-amber-700 bg-amber-950/40 p-2 text-xs text-amber-200">
                    <p className="mb-1 font-semibold">
                      {t("sshConfigImport.identityFile.willOpenHeading", { count: filesOpenedOnConfirm.length })}
                    </p>
                    <ul className="space-y-0.5 font-mono">
                      {filesOpenedOnConfirm.map((p) => (
                        <li key={p} className="break-all">
                          {p}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </section>

              {/* Profile (§3.1.7) */}
              <section className="mb-4">
                <h3 className="mb-1 font-semibold text-slate-100">
                  {t("sshConfigImport.entriesHeading", { count: preview.entries.length })}
                </h3>
                <div className="space-y-2">
                  {preview.entries.map((e) => {
                    const st = entryState[e.index];
                    if (!st) return null;
                    return (
                      <div
                        key={e.index}
                        className={`rounded border p-3 ${st.selected ? "border-slate-700 bg-slate-800/60" : "border-slate-800 bg-slate-900/40 opacity-60"}`}
                      >
                        <div className="mb-2 flex items-center gap-2">
                          <input
                            type="checkbox"
                            checked={st.selected}
                            onChange={(ev) => setEntry(e.index, { selected: ev.target.checked })}
                          />
                          <span
                            data-testid={`entry-${e.index}-name`}
                            className="font-mono font-semibold text-slate-100"
                          >
                            {e.name}
                          </span>
                          {e.conflict && (
                            <span className="rounded bg-amber-900/60 px-1.5 py-0.5 text-xs text-amber-300">
                              {t(`sshConfigImport.conflict.${e.conflict.kind}`, { name: e.conflict.existingName })}
                            </span>
                          )}
                        </div>

                        {e.conflict && (
                          <div className="mb-2 pl-6 text-xs">
                            <label className="flex items-center gap-2">
                              {t("sshConfigImport.conflict.renameLabel")}
                              <input
                                type="text"
                                className="rounded border border-slate-600 bg-slate-950 px-1.5 py-0.5 text-slate-100"
                                placeholder={t("sshConfigImport.conflict.renamePlaceholder")}
                                value={st.renameTo}
                                onChange={(ev) => setEntry(e.index, { renameTo: ev.target.value })}
                              />
                            </label>
                            {!st.renameTo.trim() && (
                              <p className="mt-1 text-slate-500">{t("sshConfigImport.conflict.skipHint")}</p>
                            )}
                          </div>
                        )}

                        <dl className="grid grid-cols-[max-content_1fr] gap-x-2 gap-y-0.5 pl-6 text-xs">
                          <dt className="text-slate-500">{t("sshConfigImport.field.host")}</dt>
                          <dd>
                            {e.host.value}{" "}
                            <span className="text-slate-500">— {originLabel(t, e.host.origin)}</span>
                          </dd>
                          <dt className="text-slate-500">{t("sshConfigImport.field.port")}</dt>
                          <dd>
                            {e.port.value}{" "}
                            <span className="text-slate-500">— {originLabel(t, e.port.origin)}</span>
                          </dd>
                          <dt className="text-slate-500">{t("sshConfigImport.field.username")}</dt>
                          <dd>
                            {e.username.value || t("sshConfigImport.field.emptyValue")}{" "}
                            <span className="text-slate-500">— {originLabel(t, e.username.origin)}</span>
                          </dd>
                          {e.jumpHost && (
                            <>
                              <dt className="text-slate-500">{t("sshConfigImport.field.jumpHost")}</dt>
                              <dd>
                                {e.jumpHost.name}{" "}
                                <span className="text-slate-500">
                                  —{" "}
                                  {e.jumpHost.existing
                                    ? t("sshConfigImport.jumpHost.existing")
                                    : t("sshConfigImport.jumpHost.planned")}
                                </span>
                              </dd>
                            </>
                          )}
                        </dl>

                        {e.identityFile && (
                          <div className="mt-2 pl-6 text-xs">
                            <span className="text-slate-500">{t("sshConfigImport.field.identityFile")}: </span>
                            <span className="font-mono break-all">{e.identityFile.path}</span>{" "}
                            <span className="text-slate-500">— {originLabel(t, e.identityFile.origin)}</span>
                            {!e.identityFile.usableWhenConnecting && (
                              <span className="ml-2 rounded bg-amber-900/60 px-1.5 py-0.5 text-amber-300">
                                {t("sshConfigImport.identityFile.notUsable")}
                              </span>
                            )}
                            <div className="mt-1 flex gap-3">
                              {(["keepAsFile", "intoKeychain", "drop"] as const).map((mode) => (
                                <label key={mode} className="flex items-center gap-1">
                                  <input
                                    type="radio"
                                    name={`identity-mode-${e.index}`}
                                    checked={effectiveMode(e.index) === mode}
                                    onChange={() => setEntry(e.index, { identityModeOverride: mode })}
                                  />
                                  {t(`sshConfigImport.identityFile.mode.${mode}`)}
                                </label>
                              ))}
                            </div>
                          </div>
                        )}

                        {e.tags.length > 0 && (
                          <div className="mt-2 flex flex-wrap gap-1 pl-6">
                            {e.tags.map((tag) => {
                              const dropped = st.droppedTags.has(tag.tag);
                              const flagged = tag.matchedRules.length > 0;
                              return (
                                <label
                                  key={tag.tag}
                                  className={`flex items-center gap-1 rounded px-1.5 py-0.5 text-xs ${
                                    tag.isLiteral
                                      ? "border border-red-600 bg-red-950/50 text-red-200"
                                      : flagged
                                        ? "border border-amber-600 bg-amber-950/40 text-amber-200"
                                        : "border border-slate-600 bg-slate-900 text-slate-300"
                                  }`}
                                  title={
                                    tag.isLiteral
                                      ? t("sshConfigImport.tag.literalHint")
                                      : flagged
                                        ? t("sshConfigImport.tag.matchesRuleHint", { count: tag.matchedRules.length })
                                        : undefined
                                  }
                                >
                                  <input
                                    type="checkbox"
                                    checked={!dropped}
                                    onChange={() => toggleTag(e.index, tag.tag)}
                                  />
                                  {tag.isLiteral && <span aria-hidden>⚠</span>}
                                  {tag.tag}
                                  {flagged && <span>({tag.matchedRules.length})</span>}
                                </label>
                              );
                            })}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </section>

              {/* Nicht übernommene Direktiven (§3.1.5) */}
              {preview.skipped.length > 0 && (
                <section className="mb-2">
                  <h3 className="mb-1 font-semibold text-slate-100">
                    {t("sshConfigImport.skippedHeading", { count: preview.skipped.length })}
                  </h3>
                  <ul className="space-y-0.5 font-mono text-xs text-slate-400">
                    {preview.skipped.map((s, i) => (
                      <li key={i}>
                        {s.file}:{s.line} — {s.directive ?? t("sshConfigImport.skipped.unreadableLine")}
                        {" — "}
                        {t(`sshConfigImport.skipReason.${s.reason}`)}
                        {s.entry && ` (${s.entry})`}
                      </li>
                    ))}
                  </ul>
                </section>
              )}
            </>
          )}
        </div>

        <div className="flex justify-end gap-2 border-t border-slate-700 p-4">
          <button
            type="button"
            onClick={onClose}
            disabled={applying}
            className="rounded border border-slate-600 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-800"
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            onClick={handleConfirm}
            disabled={!preview || applying || loading}
            className="rounded bg-indigo-600 px-3 py-1.5 text-sm font-semibold text-slate-950 hover:bg-indigo-500 disabled:opacity-50"
          >
            {applying ? t("sshConfigImport.applying") : t("sshConfigImport.confirm")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
