import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { characterCount } from "../lib/passwordLength";
import { clipboardWriteText } from "../api/api";
import { TECHNICAL_INPUT_PROPS } from "../lib/technicalInputProps";
import { Button, DialogFrame, Field, FIELD_CONTROL_CLASS, Icon, StatusSquare } from "./ui";

/** K5: minimum length; the backend checks the same limit. */
export const MIN_BACKUP_PASSPHRASE = 12;

/** The code in groups, as the backend formats it (dashes or spaces). */
export function codeGroups(code: string): string[] {
  return code.replace(/[\s-]/g, "").match(/.{1,5}/g) ?? [];
}

interface RecoveryCodeDialogProps {
  code: string;
  onContinue: () => void;
  onCancel: () => void;
}

/** K2/K6: shows the recovery code once. "Continue" stays off until the user
 * ticks that the code was noted. The code lives only in this dialog's props. */
export function RecoveryCodeDialog({ code, onContinue, onCancel }: RecoveryCodeDialogProps) {
  const { t } = useTranslation();
  const [noted, setNoted] = useState(false);
  const [copied, setCopied] = useState<"idle" | "copied" | "failed">("idle");
  const checkRef = useRef<HTMLInputElement>(null);
  const copy = async () => {
    try {
      await clipboardWriteText(codeGroups(code).join("-"));
      setCopied("copied");
    } catch {
      setCopied("failed");
    }
  };
  return (
    <DialogFrame
      layer="app"
      icon={<Icon name="key" />}
      title={t("backup.code.title")}
      onClose={onCancel}
      size="md"
      actions={
        <>
          <Button onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button variant="primary" disabled={!noted} onClick={onContinue}>
            {t("backup.code.continue")}
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <p className="flex items-start gap-2 border border-warning-border-strong bg-warning-subtle/40 p-2 text-warning-fg-softer"><StatusSquare tone="warning" className="mt-1" /><span>{t("backup.code.warning")}</span></p>
        <p
          data-testid="backup-recovery-code"
          className="flex flex-wrap gap-x-3 gap-y-1 border border-border-strong bg-surface-sunken p-3 font-mono text-base text-fg"
        >
          {codeGroups(code).map((group, i) => (
            <span key={i}>{group}</span>
          ))}
        </p>
        <div className="flex items-center gap-2">
          <Button size="sm" onClick={copy}>
            {t("backup.code.copy")}
          </Button>
          {copied === "copied" && <span className="text-xs text-success-fg">{t("about.copied")}</span>}
          {copied === "failed" && <span className="text-xs text-danger-fg-soft">{t("backup.code.copyFailed")}</span>}
        </div>
        <label className="flex items-start gap-3 narrow:min-h-11">
          <input
            ref={checkRef}
            type="checkbox"
            className="switch"
            checked={noted}
            onChange={(e) => setNoted(e.target.checked)}
          />
          <span>{t("backup.code.confirm")}</span>
        </label>
      </div>
    </DialogFrame>
  );
}

interface PassphraseDialogProps {
  onSave: (passphrase: string) => Promise<void>;
  onCancel: () => void;
}

/** K5: twice, at least 12 characters, with the warning. The passphrase is
 * cleared after every attempt. */
export function PassphraseDialog({ onSave, onCancel }: PassphraseDialogProps) {
  const { t } = useTranslation();
  const [value, setValue] = useState("");
  const [repeated, setRepeated] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const tooShort = value !== "" && characterCount(value) < MIN_BACKUP_PASSPHRASE;
  const mismatch = repeated !== "" && value !== repeated;
  const valid = characterCount(value) >= MIN_BACKUP_PASSPHRASE && value === repeated;
  const submit = async () => {
    if (!valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSave(value);
    } catch (err) {
      setError(err instanceof Error ? err.message : t("backup.errors.failed"));
    } finally {
      setValue("");
      setRepeated("");
      setBusy(false);
    }
  };
  return (
    <DialogFrame
      layer="app"
      icon={<Icon name="lock" />}
      title={t("backup.passphrase.title")}
      onClose={onCancel}
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
      actions={
        <>
          <Button type="button" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" variant="primary" disabled={!valid || busy}>
            {t("backup.passphrase.save")}
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <p className="flex items-start gap-2 border border-warning-border-strong bg-warning-subtle/40 p-2 text-warning-fg-softer"><StatusSquare tone="warning" className="mt-1" /><span>{t("backup.passphrase.warning")}</span></p>
        <Field
          label={t("backup.passphrase.label")}
          help={t("backup.passphrase.help", { count: MIN_BACKUP_PASSPHRASE })}
          error={tooShort ? t("backup.passphrase.tooShort", { count: MIN_BACKUP_PASSPHRASE }) : undefined}
        >
          {(p) => (
            <input
              {...p}
              {...TECHNICAL_INPUT_PROPS}
              type="password"
              autoComplete="new-password"
              className={FIELD_CONTROL_CLASS}
              value={value}
              onChange={(e) => setValue(e.target.value)}
            />
          )}
        </Field>
        <Field label={t("backup.passphrase.repeat")} error={mismatch ? t("backup.passphrase.mismatch") : undefined}>
          {(p) => (
            <input
              {...p}
              {...TECHNICAL_INPUT_PROPS}
              type="password"
              autoComplete="new-password"
              className={FIELD_CONTROL_CLASS}
              value={repeated}
              onChange={(e) => setRepeated(e.target.value)}
            />
          )}
        </Field>
        {error && <p role="alert" className="text-sm text-danger-fg-soft">{error}</p>}
      </div>
    </DialogFrame>
  );
}
