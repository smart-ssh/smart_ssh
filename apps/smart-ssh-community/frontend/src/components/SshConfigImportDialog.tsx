import type { TranslateFn } from "../i18nTypes";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { applySshConfigImport, commandErrorCode, commandErrorMessage, previewSshConfigImport } from "../api/api";
import { showToast } from "../buses/toastBus";
import type {
  SshConfigEntryChoice,
  SshConfigIdentityMode,
  SshConfigImportPreviewDto,
  SshConfigPreviewEntryDto,
  SshConfigPreviewTagDto,
} from "../api/types";
import { Button, DialogFrame, Icon } from "./ui";
import { TECHNICAL_INPUT_PROPS } from "../lib/technicalInputProps";

interface SshConfigImportDialogProps {
  onClose: () => void;
  /** Nach einem erfolgreichen `apply` — die Sidebar/Verwalten-Ansicht lädt
   * damit die neuen Server/Gruppen nach. */
  onImported: () => void;
}

/** Spec 0075, §5.2a/§9 — ob ein Schlagwort standardmäßig angewählt bleibt.
 * Die Vorgabe berechnet allein das Backend (Import-Plan in `core`) und
 * liefert sie als `defaultSelected` im DTO — hier wird sie nur übernommen,
 * nicht neu abgeleitet, damit Vorschau und ein `apply` ohne Wahl nie
 * auseinanderlaufen. Der Nutzer kann in beide Richtungen umwählen
 * (`toggleTag`). */
function defaultTagSelected(tag: SshConfigPreviewTagDto): boolean {
  return tag.defaultSelected;
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

const FILE_STATUS_ICON = {
  read: "file-check",
  alreadyRead: "file-check",
  notSshConfig: "file-x",
  unreadable: "file-x",
} as const;

/** Resulting folder tree as a flat, depth-ordered list: each group followed
 * by its servers and then its subgroups. */
type TreeRow =
  | { kind: "group"; key: string; depth: number; name: string }
  | { kind: "server"; key: string; depth: number; entry: SshConfigPreviewEntryDto };

function buildTreeRows(preview: SshConfigImportPreviewDto): TreeRow[] {
  const rows: TreeRow[] = [];
  const walk = (parent: number | null, depth: number) => {
    preview.groups.forEach((g, i) => {
      if (g.parent !== parent) return;
      rows.push({ kind: "group", key: `g${i}`, depth, name: g.name });
      for (const e of preview.entries) {
        if (e.group === i) rows.push({ kind: "server", key: `e${e.index}`, depth: depth + 1, entry: e });
      }
      walk(i, depth + 1);
    });
  };
  walk(null, 0);
  return rows;
}

function originLabel(
  t: TranslateFn,
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
      .catch(
        (err) =>
          !cancelled &&
          // Spec 0109, T3: a backup file belongs to "Data and backup".
          setError(commandErrorCode(err) === "SSH_CONFIG_IS_BACKUP" ? t("sshConfigImport.isBackup") : commandErrorMessage(err)),
      )
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- load the preview once on mount
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
    // eslint-disable-next-line react-hooks/exhaustive-deps -- effectiveMode derives from entryState and globalIdentityMode, both listed
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

  const selectedCount = preview ? preview.entries.filter((e) => entryState[e.index]?.selected).length : 0;

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
      // spec-reviewer-Fund, Runde 1: vorher stand hier nur die Anzahl —
      // §3.1.9 (b) verlangt aber, dass die Meldung **sagt, warum** ein
      // Eintrag zurückfiel bzw. **dass** eine Passphrase nachzutragen ist,
      // je Eintrag.
      const parts = [
        t("sshConfigImport.result.success", {
          created: outcome.createdServers,
          groups: outcome.createdGroups,
          skipped: outcome.skippedConflicts,
        }),
      ];
      if (outcome.identityFallbacks.length > 0) {
        const detail = outcome.identityFallbacks
          .map((f) => `${f.entry} (${t(`sshConfigImport.identityFile.fallbackReason.${f.reason}`)})`)
          .join(", ");
        parts.push(
          t("sshConfigImport.result.fallbacksDetail", {
            count: outcome.identityFallbacks.length,
            detail,
          }),
        );
      }
      if (outcome.identityEncrypted.length > 0) {
        const detail = outcome.identityEncrypted.map((e) => e.entry).join(", ");
        parts.push(
          t("sshConfigImport.result.encryptedDetail", {
            count: outcome.identityEncrypted.length,
            detail,
          }),
        );
      }
      showToast({ kind: "success", message: parts.join(" ") });
      onImported();
      onClose();
    } catch (err) {
      setError(commandErrorMessage(err));
      setApplying(false);
    }
  };

  return (
    <DialogFrame
      layer="app"
      size="xl"
      title={t("sshConfigImport.dialogTitle")}
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose} disabled={applying}>
            {t("common.cancel")}
          </Button>
          <Button variant="primary" onClick={handleConfirm} disabled={!preview || applying || loading}>
            {applying
              ? t("sshConfigImport.applying")
              : preview
                ? t("sshConfigImport.confirmCount", { count: selectedCount })
                : t("sshConfigImport.confirm")}
          </Button>
        </>
      }
    >
      <div>
          {loading && <p>{t("common.loading")}</p>}
          {error && <p className="mb-3 rounded border border-danger-border-strong bg-danger-subtle/50 p-2 text-danger-fg-soft">{error}</p>}

          {preview && (
            <>
              {/* Three parts: files read (§3.1.4), resulting folder structure
               * (§4.5), servers (§3.1.7); stacked when the window is narrow. */}
              <div className="mb-4 grid grid-cols-1 gap-4 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,2.4fr)]">
                <section aria-labelledby="ssh-import-files">
                  <h3 id="ssh-import-files" className="mb-2 font-mono text-xs uppercase tracking-wider text-fg-subtle">{t("sshConfigImport.filesHeading")}</h3>
                  <ul className="space-y-1 font-mono text-xs">
                    {preview.files.map((f) => (
                      <li key={f.path} className="flex items-start gap-2" style={{ paddingLeft: `${f.depth * 14}px` }}>
                        <Icon
                          name={FILE_STATUS_ICON[f.status]}
                          className={`mt-0.5 shrink-0 ${f.status === "read" ? "text-success-fg" : "text-fg-subtle"}`}
                        />
                        <span className={`min-w-0 break-all ${f.status === "read" ? "" : "text-fg-subtle"}`}>{f.path}</span>
                        <span className="ml-auto shrink-0 text-fg-subtle">{t(`sshConfigImport.fileStatus.${f.status}`)}</span>
                      </li>
                    ))}
                  </ul>
                </section>

                <section aria-labelledby="ssh-import-tree">
                  <h3 id="ssh-import-tree" className="mb-2 font-mono text-xs uppercase tracking-wider text-fg-subtle">{t("sshConfigImport.groupsHeading")}</h3>
                  <ul className="space-y-0.5 text-xs">
                    {buildTreeRows(preview).map((row) => (
                      <li
                        key={row.key}
                        className={`flex items-center gap-1.5 ${row.depth > 0 ? "border-l border-border-strong" : ""} ${row.kind === "server" && !entryState[row.entry.index]?.selected ? "opacity-50" : ""}`}
                        style={{ marginLeft: `${Math.max(row.depth - 1, 0) * 14}px`, paddingLeft: row.depth > 0 ? "8px" : 0 }}
                      >
                        {row.kind === "group" ? (
                          <>
                            <Icon name="folder" className="shrink-0" />
                            <span className="min-w-0 break-words">{row.name}</span>
                          </>
                        ) : (
                          <>
                            <Icon name="server" className="shrink-0 text-fg-subtle" />
                            <span className="min-w-0 break-words font-mono">{row.entry.name}</span>
                          </>
                        )}
                      </li>
                    ))}
                  </ul>
                </section>

                <section aria-labelledby="ssh-import-servers" className="min-w-0">
                  <h3 id="ssh-import-servers" className="mb-2 font-mono text-xs uppercase tracking-wider text-fg-subtle">
                    {t("sshConfigImport.entriesHeading", { count: preview.entries.length })}
                  </h3>
                  <div className="overflow-x-auto overflow-y-hidden rounded border border-border">
                    <table className="w-full text-left text-xs">
                      <thead className="font-mono uppercase tracking-wider text-fg-subtle">
                        <tr>
                          <th scope="col" className="w-8 px-2 py-1.5" aria-label={t("sshConfigImport.field.include")} title={t("sshConfigImport.field.include")}><Icon name="check" /></th>
                          <th scope="col" className="px-2 py-1.5 font-normal">{t("sshConfigImport.field.name")}</th>
                          <th scope="col" className="px-2 py-1.5 font-normal">{t("sshConfigImport.field.host")}</th>
                          <th scope="col" className="px-2 py-1.5 font-normal">{t("sshConfigImport.field.port")}</th>
                          <th scope="col" className="px-2 py-1.5 font-normal">{t("sshConfigImport.field.username")}</th>
                          <th scope="col" className="px-2 py-1.5 font-normal">{t("sshConfigImport.field.jumpHost")}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {preview.entries.map((e) => {
                          const st = entryState[e.index];
                          if (!st) return null;
                          return (
                            <tr
                              key={e.index}
                              className={`border-t border-border ${e.conflict ? "bg-warning-subtle/40" : ""} ${st.selected ? "" : "opacity-60"}`}
                            >
                              <td className="px-2 py-1.5">
                                <input
                                  type="checkbox"
                                  aria-label={e.name}
                                  checked={st.selected}
                                  onChange={(ev) => setEntry(e.index, { selected: ev.target.checked })}
                                />
                              </td>
                              <td data-testid={`entry-${e.index}-name`} className="px-2 py-1.5 font-mono text-fg">{e.name}</td>
                              <td className="px-2 py-1.5 font-mono">{e.host.value}</td>
                              <td className="px-2 py-1.5 font-mono">{e.port.value}</td>
                              <td className="px-2 py-1.5 font-mono">{e.username.value || t("sshConfigImport.field.emptyValue")}</td>
                              <td className="px-2 py-1.5">
                                {e.conflict ? (
                                  <span className="text-warning-fg-soft">
                                    {t(`sshConfigImport.conflict.${e.conflict.kind}`, { name: e.conflict.existingName })}
                                  </span>
                                ) : e.jumpHost ? (
                                  <>
                                    {e.jumpHost.name}{" "}
                                    <span className="text-fg-subtle">
                                      ·{" "}
                                      {e.jumpHost.existing
                                        ? t("sshConfigImport.jumpHost.existing")
                                        : t("sshConfigImport.jumpHost.planned")}
                                    </span>
                                  </>
                                ) : null}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                  {preview.entries.map((e) => {
                    const st = entryState[e.index];
                    if (!st || !e.conflict) return null;
                    return (
                      <div key={e.index} className="mt-3 text-xs">
                        <label className="flex flex-wrap items-center gap-2">
                          <span>{e.name}: {t("sshConfigImport.conflict.renameLabel")}</span>
                          <input
                            {...TECHNICAL_INPUT_PROPS}
                            type="text"
                            className="rounded border border-border-strong bg-surface-sunken px-1.5 py-1 font-mono text-fg"
                            placeholder={t("sshConfigImport.conflict.renamePlaceholder")}
                            value={st.renameTo}
                            onChange={(ev) => setEntry(e.index, { renameTo: ev.target.value })}
                          />
                        </label>
                        {!st.renameTo.trim() && <p className="mt-1 text-fg-subtle">{t("sshConfigImport.conflict.skipHint")}</p>}
                      </div>
                    );
                  })}
                </section>
              </div>

              {/* Weg (b): globale Wahl + Dateien, die dabei aufgingen (§3.1.9) */}
              <section className="mb-4 rounded border border-border p-3">
                <h3 className="mb-2 font-semibold text-fg">{t("sshConfigImport.identityFile.modeHeading")}</h3>
                <div className="mb-2 flex flex-wrap gap-4">
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
                  <div className="rounded border border-warning-border-strong bg-warning-subtle/40 p-2 text-xs text-warning-fg-softer">
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
                    {/* §3.1.9 (b): ob EIN konkreter Schlüssel verschlüsselt
                     * ist, weiß erst das tatsächliche Lesen beim Bestätigen
                     * (§5.1 — die Vorschau öffnet nichts); dieser Hinweis
                     * ist deshalb bewusst allgemein, nicht pro Datei. */}
                    <p className="mt-1">{t("sshConfigImport.identityFile.encryptedHint")}</p>
                  </div>
                )}
              </section>

              {/* Per-entry details (§3.1.7): origin of each value, key file, tags */}
              <section className="mb-4 space-y-2">
                {preview.entries.map((e) => {
                  const st = entryState[e.index];
                  if (!st) return null;
                  return (
                    <div
                      key={e.index}
                      className={`rounded border p-3 ${st.selected ? "border-border bg-surface-raised/60" : "border-surface-raised bg-surface/40 opacity-60"}`}
                    >
                      <p className="mb-2 font-mono text-xs font-semibold text-fg">{e.name}</p>
                      <dl className="grid min-w-0 grid-cols-[max-content_minmax(0,1fr)] break-words gap-x-2 gap-y-0.5 text-xs">
                        <dt className="text-fg-subtle">{t("sshConfigImport.field.host")}</dt>
                        <dd>
                          {e.host.value} <span className="text-fg-subtle">— {originLabel(t, e.host.origin)}</span>
                        </dd>
                        <dt className="text-fg-subtle">{t("sshConfigImport.field.port")}</dt>
                        <dd>
                          {e.port.value} <span className="text-fg-subtle">— {originLabel(t, e.port.origin)}</span>
                        </dd>
                        <dt className="text-fg-subtle">{t("sshConfigImport.field.username")}</dt>
                        <dd>
                          {e.username.value || t("sshConfigImport.field.emptyValue")}{" "}
                          <span className="text-fg-subtle">— {originLabel(t, e.username.origin)}</span>
                        </dd>
                        {e.jumpHost && (
                          <>
                            <dt className="text-fg-subtle">{t("sshConfigImport.field.jumpHost")}</dt>
                            <dd data-testid={`entry-${e.index}-jump`}>
                              {e.jumpHost.name}{" "}
                              <span className="text-fg-subtle">
                                — {e.jumpHost.existing ? t("sshConfigImport.jumpHost.existing") : t("sshConfigImport.jumpHost.planned")}
                              </span>
                            </dd>
                          </>
                        )}
                      </dl>

                      {e.identityFile && (
                        <div className="mt-2 text-xs">
                          <span className="text-fg-subtle">{t("sshConfigImport.field.identityFile")}: </span>
                          <span className="font-mono break-all">{e.identityFile.path}</span>{" "}
                          <span className="text-fg-subtle">— {originLabel(t, e.identityFile.origin)}</span>
                          {!e.identityFile.usableWhenConnecting && (
                            <span className="ml-2 rounded bg-warning-subtle-hover/60 px-1.5 py-0.5 text-warning-fg-soft">
                              {t("sshConfigImport.identityFile.notUsable")}
                            </span>
                          )}
                          <div className="mt-1 flex flex-wrap gap-3">
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
                        <div className="mt-2 flex flex-wrap gap-1">
                          {e.tags.map((tag) => {
                            const dropped = st.droppedTags.has(tag.tag);
                            const flagged = tag.matchedRules.length > 0;
                            // §5.2a verlangt, die betroffene Regel zu
                            // nennen — mindestens ihre Wirkung (Allow
                            // kann Confirm→Allow heben, Deny/Confirm
                            // ändern nichts an der bestehenden Stufe).
                            const actions = [...new Set(tag.matchedRules.map((r) => r.action))];
                            const actionLabels = actions
                              .map((a) => t(`sshConfigImport.tag.action.${a}`))
                              .join(", ");
                            return (
                              <label
                                key={tag.tag}
                                className={`flex items-center gap-1 rounded px-1.5 py-0.5 text-xs ${
                                  tag.isLiteral
                                    ? "border border-danger bg-danger-subtle/50 text-danger-fg-softer"
                                    : flagged
                                      ? "border border-warning bg-warning-subtle/40 text-warning-fg-softer"
                                      : "border border-border-strong bg-surface text-fg-muted"
                                }`}
                                title={
                                  tag.isLiteral
                                    ? t("sshConfigImport.tag.literalHint")
                                    : flagged
                                      ? t("sshConfigImport.tag.matchesRuleHint", {
                                          count: tag.matchedRules.length,
                                          actions: actionLabels,
                                        })
                                      : undefined
                                }
                              >
                                <input
                                  type="checkbox"
                                  checked={!dropped}
                                  onChange={() => toggleTag(e.index, tag.tag)}
                                />
                                {tag.isLiteral && <Icon name="warn" />}
                                {tag.tag}
                                {flagged && <span>({actionLabels})</span>}
                              </label>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  );
                })}
              </section>

              {/* Nicht übernommene Direktiven (§3.1.5) */}
              {preview.skipped.length > 0 && (
                <section className="mb-2">
                  <h3 className="mb-1 font-semibold text-fg">
                    {t("sshConfigImport.skippedHeading", { count: preview.skipped.length })}
                  </h3>
                  <ul className="space-y-0.5 font-mono text-xs text-fg-subtle">
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
    </DialogFrame>
  );
}
