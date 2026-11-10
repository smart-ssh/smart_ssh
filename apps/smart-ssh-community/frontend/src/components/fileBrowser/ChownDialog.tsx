import { isRealDir } from "../../lib/remotePath";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type RemoteEntryDto } from "../../api/types";
import { Button, DialogFrame, Field, FIELD_CONTROL_CLASS } from "../ui";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";

/** Issue #654: "Change owner…" dialog. User and group are prefilled with the
 * current name (or number); a field the user leaves as it was is sent as
 * `null` so that side stays untouched. Names or numbers are accepted. */
export function ChownDialog({
  entry,
  modeNote,
  onCancel,
  onConfirm,
}: {
  entry: RemoteEntryDto;
  modeNote: string | null;
  onCancel: () => void;
  onConfirm: (owner: string | null, group: string | null, recursive: boolean) => void;
}) {
  const { t } = useTranslation();
  const initialOwner = entry.owner ?? (entry.uid !== null ? String(entry.uid) : "");
  const initialGroup = entry.group ?? (entry.gid !== null ? String(entry.gid) : "");
  const [owner, setOwner] = useState(initialOwner);
  const [group, setGroup] = useState(initialGroup);
  const [recursive, setRecursive] = useState(false);
  const ownerRef = useRef<HTMLInputElement>(null);
  const side = (value: string, initial: string) => {
    const v = value.trim();
    return v === "" || v === initial ? null : v;
  };
  const ownerChange = side(owner, initialOwner);
  const groupChange = side(group, initialGroup);
  const nothingToChange = ownerChange === null && groupChange === null;

  return (
    <DialogFrame
      layer="sessionTab"
      size="sm"
      title={t("fileBrowser.chown.title")}
      onClose={onCancel}
      initialFocusRef={ownerRef}
      onSubmit={(e) => {
        e.preventDefault();
        if (nothingToChange) return;
        onConfirm(ownerChange, groupChange, recursive && isRealDir(entry));
      }}
      actions={
        <>
          <Button onClick={onCancel}>{t("fileBrowser.cancel")}</Button>
          <Button type="submit" variant="primary" disabled={nothingToChange}>
            {t("fileBrowser.apply")}
          </Button>
        </>
      }
    >
      <p className="mb-3 font-mono text-xs break-all text-fg-subtle">{entry.path}</p>
      {modeNote && (
        <p className="mb-3 border border-warning/60 bg-warning-subtle/40 px-2 py-1 text-xs text-warning-fg-soft">
          ⚠ {modeNote}
        </p>
      )}
      <Field label={t("fileBrowser.chown.owner")} className="mb-3">
        {(control) => (
          <input
            {...TECHNICAL_INPUT_PROPS}
            {...control}
            ref={ownerRef}
            value={owner}
            onChange={(e) => setOwner(e.target.value)}
            className={`${FIELD_CONTROL_CLASS} font-mono`}
          />
        )}
      </Field>
      <Field label={t("fileBrowser.chown.group")} className="mb-3">
        {(control) => (
          <input
            {...TECHNICAL_INPUT_PROPS}
            {...control}
            value={group}
            onChange={(e) => setGroup(e.target.value)}
            className={`${FIELD_CONTROL_CLASS} font-mono`}
          />
        )}
      </Field>
      <p className="mb-3 text-xs text-fg-subtle">{t("fileBrowser.chown.hint")}</p>
      {isRealDir(entry) && (
        <label className="flex items-start gap-2 text-sm text-warning-fg-soft">
          <input
            type="checkbox"
            checked={recursive}
            onChange={(e) => setRecursive(e.target.checked)}
            className="mt-1 focus-ring-flush"
          />
          <span>
            <strong>{t("fileBrowser.chown.recursive")}</strong> {t("fileBrowser.chown.recursiveWarning")}
          </span>
        </label>
      )}
    </DialogFrame>
  );
}
