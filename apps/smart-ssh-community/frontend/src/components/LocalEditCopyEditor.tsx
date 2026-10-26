import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { readEditCopyText, writeEditCopyText } from "../api";
import { commandErrorMessage } from "../api";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";
import { Button, DialogFrame, FIELD_CONTROL_CLASS } from "./ui";

interface Props {
  sessionId: string;
  /** Edit copy of the session (never an arbitrary local path). */
  localPath: string;
  fileName: string;
  onClose: () => void;
}

/** Issue #679: the app's own text editor for an edit copy, used where no
 * external program can be started (Android). Saving writes the edit copy;
 * the usual change detection then offers the upload to the server. */
export function LocalEditCopyEditor({ sessionId, localPath, fileName, onClose }: Props) {
  const { t } = useTranslation();
  const [text, setText] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    readEditCopyText(sessionId, localPath)
      .then((content) => {
        if (!cancelled) setText(content);
      })
      .catch((err) => {
        if (!cancelled) setError(commandErrorMessage(err));
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, localPath]);

  const save = () => {
    if (text === null) return;
    setSaving(true);
    setError(null);
    writeEditCopyText(sessionId, localPath, text)
      .then(onClose)
      .catch((err) => setError(commandErrorMessage(err)))
      .finally(() => setSaving(false));
  };

  return (
    <DialogFrame
      layer="sessionTab"
      size="xl"
      title={fileName}
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>{t("common.close")}</Button>
          <Button variant="primary" disabled={text === null || saving} onClick={save}>
            {saving ? t("common.saving") : t("common.save")}
          </Button>
        </>
      }
    >
      {error && (
        <p role="alert" className="mb-2 text-xs text-danger-fg">
          {error}
        </p>
      )}
      <textarea
        aria-label={fileName}
        {...TECHNICAL_INPUT_PROPS}
        disabled={text === null}
        value={text ?? ""}
        onChange={(e) => setText(e.target.value)}
        className={`${FIELD_CONTROL_CLASS} min-h-64 w-full flex-1 font-mono text-xs`}
      />
    </DialogFrame>
  );
}
