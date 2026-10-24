import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, createBackupKeyWithCode, newBackupCode, setBackupPassphrase } from "../api";
import { Button, DialogFrame } from "./ui";
import { PassphraseDialog, RecoveryCodeDialog } from "./BackupDialogs";

/** Set while a code was shown but the user did not confirm having noted it
 * (K2): the next export then asks for a new code instead of using one nobody
 * has seen. Holds no secret. */
const UNCONFIRMED_KEY = "smart-ssh.backupCodeUnconfirmed";

export function backupCodeUnconfirmed(): boolean {
  try {
    return localStorage.getItem(UNCONFIRMED_KEY) === "1";
  } catch {
    return false;
  }
}
function markUnconfirmed(value: boolean) {
  try {
    if (value) localStorage.setItem(UNCONFIRMED_KEY, "1");
    else localStorage.removeItem(UNCONFIRMED_KEY);
  } catch {
    // Storage unavailable: the user still has to tick the box to continue.
  }
}

export type KeySetupMode = "choose" | "newCode" | "passphrase";

interface Props {
  /** `choose`: no key yet (K2) — code or passphrase. `newCode`: K6. `passphrase`: K5. */
  mode: KeySetupMode;
  /** The key is set up and, for a code, the user confirmed having noted it. */
  onDone: () => void;
  onCancel: () => void;
}

/** K2, K5, K6. The code exists only in this component's state while its dialog is open. */
export function BackupKeySetup({ mode, onDone, onCancel }: Props) {
  const { t } = useTranslation();
  const [stage, setStage] = useState<"choose" | "code" | "passphrase">(mode === "passphrase" ? "passphrase" : mode === "newCode" ? "code" : "choose");
  const [code, setCode] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [started, setStarted] = useState(false);

  const showCode = async (create: () => Promise<string>) => {
    setError(null);
    try {
      const fresh = await create();
      markUnconfirmed(true);
      setCode(fresh);
      setStage("code");
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };
  // `newCode` starts directly with a fresh code.
  if (mode === "newCode" && !started) {
    setStarted(true);
    void showCode(newBackupCode);
  }

  if (stage === "passphrase") {
    return (
      <PassphraseDialog
        onCancel={onCancel}
        onSave={async (passphrase) => {
          try {
            await setBackupPassphrase(passphrase);
          } catch (err) {
            throw new Error(commandErrorMessage(err));
          }
          onDone();
        }}
      />
    );
  }
  if (stage === "code" && code !== null) {
    return (
      <RecoveryCodeDialog
        code={code}
        onCancel={() => {
          setCode(null);
          onCancel();
        }}
        onContinue={() => {
          markUnconfirmed(false);
          setCode(null);
          onDone();
        }}
      />
    );
  }
  if (mode === "newCode") {
    return error ? (
      <DialogFrame layer="app" title={t("backup.code.title")} onClose={onCancel} actions={<Button onClick={onCancel}>{t("common.close")}</Button>}>
        <p role="alert" className="text-red-300">{error}</p>
      </DialogFrame>
    ) : null;
  }
  return (
    <DialogFrame
      layer="app"
      title={t("backup.setup.title")}
      onClose={onCancel}
      actions={
        <Button className="narrow:min-h-11" onClick={onCancel}>
          {t("common.cancel")}
        </Button>
      }
    >
      <div className="space-y-3">
        <p>{t("backup.setup.intro")}</p>
        <div className="flex flex-col gap-2">
          <Button variant="primary" className="narrow:min-h-11" onClick={() => void showCode(createBackupKeyWithCode)}>
            {t("backup.setup.useCode")}
          </Button>
          <Button className="narrow:min-h-11" onClick={() => setStage("passphrase")}>
            {t("backup.setup.usePassphrase")}
          </Button>
        </div>
        {error && <p role="alert" className="text-red-300">{error}</p>}
      </div>
    </DialogFrame>
  );
}
