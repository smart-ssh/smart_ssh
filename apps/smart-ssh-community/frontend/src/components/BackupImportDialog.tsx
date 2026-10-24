import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  backupKeyStatus,
  cancelBackupImport,
  commandErrorCode,
  commandErrorMessage,
  confirmBackupImport,
  pickBackupImportFile,
  startBackupImport,
} from "../api";
import type { BackupCountsDto, BackupImportPreviewDto, BackupImportSummaryDto } from "../api";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";
import { Button, DialogFrame, Field, FIELD_CONTROL_CLASS } from "./ui";
import { BackupKeySetup } from "./BackupKeySetup";

interface Props {
  onClose: () => void;
  /** All views reload after a restore (I8). */
  onRestored: () => void;
}

const ERROR_KEYS: Record<string, string> = {
  BACKUP_CANNOT_OPEN: "backup.errors.cannotOpen",
  BACKUP_CODE_TYPO: "backup.errors.typo",
  BACKUP_NOT_A_BACKUP: "backup.errors.notBackup",
  BACKUP_NEWER_VERSION: "backup.errors.newerVersion",
};

const COUNT_ROWS: (keyof BackupCountsDto)[] = ["servers", "groups", "rules", "notes", "aiProviders"];

/** Spec 0109, §7: file → code or passphrase → counts confirmation → summary.
 * The secret is sent once and dropped from state right after the call. */
export function BackupImportDialog({ onClose, onRestored }: Props) {
  const { t } = useTranslation();
  const [fileName, setFileName] = useState<string | null>(null);
  const [kind, setKind] = useState<"code" | "passphrase">("code");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<BackupImportPreviewDto | null>(null);
  const [summary, setSummary] = useState<BackupImportSummaryDto | null>(null);
  const [needsKey, setNeedsKey] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const finished = useRef(false);

  useEffect(() => {
    let cancelled = false;
    pickBackupImportFile(t("backup.import.pickTitle"))
      .then((name) => {
        if (cancelled) return;
        if (name === null) onClose();
        else setFileName(name);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(commandErrorMessage(err));
        setFileName("");
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const describe = (err: unknown) => {
    const key = ERROR_KEYS[commandErrorCode(err) ?? ""];
    return key ? t(key) : t("backup.errors.failed");
  };

  const close = () => {
    if (!finished.current) void cancelBackupImport().catch(() => undefined);
    onClose();
  };

  const open = async () => {
    if (busy || secret.trim() === "") return;
    const sent = secret;
    setBusy(true);
    setError(null);
    setSecret("");
    try {
      const result = await startBackupImport(kind, sent);
      if (result.imported) {
        finished.current = true;
        setSummary(result.imported);
      } else if (result.preview) {
        setPreview(result.preview);
      }
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      // I4.1: the safety backup needs the device's backup key.
      if (!(await backupKeyStatus()).exists) {
        setNeedsKey(true);
        return;
      }
      const done = await confirmBackupImport();
      finished.current = true;
      setSummary(done);
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  if (fileName === null) return null;

  if (needsKey) {
    return (
      <BackupKeySetup
        mode="choose"
        onCancel={() => setNeedsKey(false)}
        onDone={() => {
          setNeedsKey(false);
          void confirm();
        }}
      />
    );
  }

  if (summary) {
    const done = () => {
      onClose();
      onRestored();
    };
    return (
      <DialogFrame
        layer="app"
        title={t("backup.summary.title")}
        onClose={done}
        actions={
          <Button variant="primary" className="narrow:min-h-11" onClick={done}>
            {t("backup.summary.done")}
          </Button>
        }
      >
        <div className="space-y-2 break-words">
          <p>{t("backup.summary.counts", { ...summary.counts })}</p>
          {summary.safetyBackup && <p>{t("backup.summary.safety", { path: summary.safetyBackup })}</p>}
          {summary.previousDataFolder && <p>{t("backup.summary.folder", { name: summary.previousDataFolder })}</p>}
          {summary.hostKeysPending && <p>{t("backup.summary.hostKeysPending")}</p>}
        </div>
      </DialogFrame>
    );
  }

  if (preview) {
    return (
      <DialogFrame
        layer="app"
        role="alertdialog"
        title={t("backup.confirm.title")}
        onClose={close}
        actions={
          <>
            <Button className="narrow:min-h-11" onClick={close} disabled={busy}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" className="narrow:min-h-11" onClick={() => void confirm()} disabled={busy}>
              {t("backup.confirm.replace")}
            </Button>
          </>
        }
      >
        <div className="space-y-3 break-words">
          <p>{t("backup.confirm.intro")}</p>
          <table className="w-full text-left">
            <thead>
              <tr>
                <th scope="col" className="font-medium">{t("backup.confirm.what")}</th>
                <th scope="col" className="font-medium">{t("backup.confirm.local")}</th>
                <th scope="col" className="font-medium">{t("backup.confirm.file")}</th>
              </tr>
            </thead>
            <tbody>
              {COUNT_ROWS.map((row) => (
                <tr key={row}>
                  <th scope="row" className="font-normal">{t(`backup.counts.${row}`)}</th>
                  <td>{preview.local[row]}</td>
                  <td>{preview.file[row]}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p>{t("backup.confirm.sessions", { count: preview.openSessions })}</p>
          <p>{t("backup.confirm.safety", { dir: preview.safetyBackupDir })}</p>
          {error && <p role="alert" className="text-red-300">{error}</p>}
        </div>
      </DialogFrame>
    );
  }

  return (
    <DialogFrame
      layer="app"
      title={t("backup.import.title")}
      onClose={close}
      initialFocusRef={inputRef}
      onSubmit={(e) => {
        e.preventDefault();
        void open();
      }}
      actions={
        <>
          <Button type="button" className="narrow:min-h-11" onClick={close}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" variant="primary" className="narrow:min-h-11" disabled={busy || secret.trim() === "" || fileName === ""}>
            {t("backup.import.open")}
          </Button>
        </>
      }
    >
      <div className="space-y-3 break-words">
        {fileName !== "" && <p>{t("backup.import.file", { name: fileName })}</p>}
        <fieldset className="flex flex-wrap gap-4">
          <legend className="sr-only">{t("backup.import.kind")}</legend>
          {(["code", "passphrase"] as const).map((k) => (
            <label key={k} className="flex items-center gap-2 narrow:min-h-11">
              <input
                type="radio"
                name="backup-secret-kind"
                checked={kind === k}
                onChange={() => {
                  setKind(k);
                  setSecret("");
                  setError(null);
                }}
              />
              {t(k === "code" ? "backup.import.useCode" : "backup.import.usePassphrase")}
            </label>
          ))}
        </fieldset>
        <Field label={t(kind === "code" ? "backup.import.codeLabel" : "backup.import.passphraseLabel")} error={error ?? undefined}>
          {(p) => (
            <input
              {...p}
              {...TECHNICAL_INPUT_PROPS}
              ref={inputRef}
              type={kind === "code" ? "text" : "password"}
              autoComplete="off"
              className={FIELD_CONTROL_CLASS}
              value={secret}
              onChange={(e) => setSecret(e.target.value)}
            />
          )}
        </Field>
      </div>
    </DialogFrame>
  );
}
