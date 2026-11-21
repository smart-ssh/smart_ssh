import { useState } from "react";
import { useTranslation } from "react-i18next";
import { backupKeyStatus, commandErrorMessage, exportEncryptedBackup } from "../api/api";
import type { BackupExportDto } from "../api/api";
import { Button, StatusSquare } from "./ui";
import { BackupImportDialog } from "./BackupImportDialog";
import { BackupKeySetup, backupCodeUnconfirmed } from "./BackupKeySetup";
import type { KeySetupMode } from "./BackupKeySetup";

interface Props {
  /** Reload every view after a restore. */
  onRestored?: () => void;
}

/** Spec 0109, T1/T4/X1: "Data and backup" → "Encrypted backup". */
export function BackupSettings({ onRestored = () => window.location.reload() }: Props) {
  const { t } = useTranslation();
  const [setup, setSetup] = useState<{ mode: KeySetupMode; then: "export" | "none" } | null>(null);
  const [importing, setImporting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<BackupExportDto | null>(null);

  const runExport = async () => {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const done = await exportEncryptedBackup();
      if (done) setResult(done);
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const startExport = async () => {
    setError(null);
    setResult(null);
    try {
      const status = await backupKeyStatus();
      if (!status.exists) setSetup({ mode: "choose", then: "export" });
      else if (backupCodeUnconfirmed() && status.hasCode) setSetup({ mode: "newCode", then: "export" });
      else await runExport();
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  return (
    <section className="space-y-4">
      <h4 className="text-label border-b border-border pb-2">{t("backup.title")}</h4>
      <p className="text-sm text-fg-muted">{t("backup.intro")}</p>
      <ul className="list-disc space-y-1 pl-5 text-sm text-fg-muted">
        <li>{t("backup.included")}</li>
        <li>{t("backup.excluded")}</li>
        <li>{t("backup.restoreInfo")}</li>
      </ul>
      <div className="flex flex-wrap gap-2">
        <Button variant="primary" disabled={busy} onClick={() => void startExport()}>
          {t("backup.export")}
        </Button>
        <Button onClick={() => setImporting(true)}>
          {t("backup.restore")}
        </Button>
        <Button onClick={() => setSetup({ mode: "newCode", then: "none" })}>
          {t("backup.newCode")}
        </Button>
        <Button onClick={() => setSetup({ mode: "passphrase", then: "none" })}>
          {t("backup.setPassphrase")}
        </Button>
      </div>
      {error && <p role="alert" className="flex items-center gap-2 border border-danger-border-strong bg-danger-subtle/50 p-2 text-sm text-danger-fg-soft"><StatusSquare tone="danger" />{error}</p>}
      {result && (
        <output className="flex items-start gap-2 break-words border border-success-border bg-success-subtle/30 p-2 text-sm text-success-fg-softer">
          <StatusSquare tone="success" className="mt-1" />
          <span>
            {t("backup.exportDone", { path: result.path })} {t("backup.exportCounts", { ...result })}
          </span>
        </output>
      )}
      {setup && (
        <BackupKeySetup
          mode={setup.mode}
          onCancel={() => setSetup(null)}
          onDone={() => {
            const next = setup.then;
            setSetup(null);
            if (next === "export") void runExport();
          }}
        />
      )}
      {importing && <BackupImportDialog onClose={() => setImporting(false)} onRestored={onRestored} />}
    </section>
  );
}
