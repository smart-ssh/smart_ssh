import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { characterCount } from "../passwordLength";
import { clipboardWriteText } from "../api";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";
import { Button, DialogFrame, Field, FIELD_CONTROL_CLASS } from "./ui";

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
      title={t("backup.code.title")}
      onClose={onCancel}
      size="md"
      actions={
        <>
          <Button className="narrow:min-h-11" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button variant="primary" className="narrow:min-h-11" disabled={!noted} onClick={onContinue}>
            {t("backup.code.continue")}
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <p className="rounded border border-amber-700 bg-amber-950/40 p-2 text-amber-200">{t("backup.code.warning")}</p>
        <p
          data-testid="backup-recovery-code"
          className="flex flex-wrap gap-x-3 gap-y-1 rounded border border-slate-600 bg-slate-950 p-3 font-mono text-base text-slate-100"
        >
          {codeGroups(code).map((group, i) => (
            <span key={i}>{group}</span>
          ))}
        </p>
        <div className="flex items-center gap-2">
          <Button size="sm" className="narrow:min-h-11" onClick={copy}>
            {t("backup.code.copy")}
          </Button>
          {copied === "copied" && <span className="text-xs text-emerald-400">{t("about.copied")}</span>}
          {copied === "failed" && <span className="text-xs text-red-300">{t("backup.code.copyFailed")}</span>}
        </div>
        <label className="flex items-start gap-2 narrow:min-h-11">
          <input
            ref={checkRef}
            type="checkbox"
            className="mt-1 h-4 w-4"
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
      title={t("backup.passphrase.title")}
      onClose={onCancel}
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
      actions={
        <>
          <Button type="button" className="narrow:min-h-11" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" variant="primary" className="narrow:min-h-11" disabled={!valid || busy}>
            {t("backup.passphrase.save")}
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <p className="rounded border border-amber-700 bg-amber-950/40 p-2 text-amber-200">{t("backup.passphrase.warning")}</p>
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
        {error && <p role="alert" className="text-sm text-red-300">{error}</p>}
      </div>
    </DialogFrame>
  );
}
