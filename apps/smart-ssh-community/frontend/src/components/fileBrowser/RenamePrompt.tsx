import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Button, DialogFrame, Field, FIELD_CONTROL_CLASS } from "../ui";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";

/** Gemeinsame kleine Eingabeaufforderung für "Umbenennen" und "Neuer
 * Ordner" — beide brauchen nur einen einzelnen Namen mit Bestätigen/
 * Abbrechen, ein voller Modal-Dialog (wie beim Löschen) wäre hier
 * überdimensioniert. */
export function RenamePrompt({
  title,
  initialValue,
  error,
  onChange,
  onCancel,
  onConfirm,
}: {
  title?: string;
  initialValue: string;
  /** Issue #570: Hinweis zu einem ungültigen Namen (ohne Backend-Aufruf). */
  error?: string | null;
  onChange: (value: string) => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  return (
    <DialogFrame
      layer="sessionTab"
      size="sm"
      title={title ?? t("fileBrowser.renamePrompt.renameTitle")}
      onClose={onCancel}
      onSubmit={(e) => {
        e.preventDefault();
        onConfirm();
      }}
      initialFocusRef={inputRef}
      actions={
        <>
          <Button onClick={onCancel}>{t("fileBrowser.cancel")}</Button>
          <Button type="submit" variant="primary">
            {t("fileBrowser.apply")}
          </Button>
        </>
      }
    >
      <Field label={t("fileBrowser.properties.name")}>
        {(control) => (
          <input
            {...TECHNICAL_INPUT_PROPS}
            {...control}
            ref={inputRef}
            value={initialValue}
            onChange={(e) => onChange(e.target.value)}
            className={`${FIELD_CONTROL_CLASS} w-full font-mono`}
          />
        )}
      </Field>
      {error && (
        <p role="alert" className="mt-2 text-xs text-danger-fg">
          {error}
        </p>
      )}
    </DialogFrame>
  );
}
