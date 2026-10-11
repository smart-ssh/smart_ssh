import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { characterCount } from "../passwordLength";
import {
  changeMasterPassword,
  commandErrorCode,
  commandErrorMessage,
  getMasterPasswordMode,
  setUpMasterPassword,
  switchToOsKeychain,
} from "../api";
import { translateErrorCode } from "../errorCodes";
import { Button, DialogFrame, Field, FIELD_CONTROL_CLASS } from "./ui";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";

/** Spec 0101, A13: Mindestlänge. Das Backend prüft dieselbe Grenze und
 * lehnt mit `MASTER_PASSWORD_REJECTED` ab; hier bleibt nur der Knopf aus. */
const MINIMUM_PASSWORD_LENGTH = 12;

/** Klarstellung 10b: Dieser Code — und nur dieser — hat eine Fortsetzung:
 * die Frage, ob ein fremder Schlüssel im Schlüsselbund ersetzt werden
 * darf. */
const KEYCHAIN_HOLDS_ANOTHER_KEY = "KEYCHAIN_HOLDS_ANOTHER_KEY";

type Mode = "password" | "keychain";

/**
 * Spec 0101, A18/A13/A15: **Wo der Schlüssel zur Datenbank liegt.**
 *
 * Zeigt den aktiven Modus und bietet den Wechsel in beide Richtungen.
 *
 * **Drei Dinge, die hier bewusst so sind:**
 *
 * 1. **Die Warnung aus A13 hat eine ausdrückliche Bestätigung** (E10): Ohne
 *    das Master-Passwort sind die Daten verloren, es gibt keine
 *    Wiederherstellung. Der Knopf bleibt aus, bis das Häkchen sitzt.
 * 2. **Der Rückweg auf den Schlüsselbund fragt, bevor er überschreibt**
 *    (Klarstellung 10b): Der erste Aufruf läuft **ohne** Bestätigung; kommt
 *    `KEYCHAIN_HOLDS_ANOTHER_KEY` zurück, erscheint die Frage, und erst ein
 *    zweiter Aufruf mit der Bestätigung ersetzt den fremden Eintrag. Lehnt
 *    der Nutzer ab, bleibt alles, wie es war — das Backend hat in diesem
 *    Fall nichts angefasst.
 * 3. **Kein Passwort bleibt im Zustand liegen:** Nach jedem Vorgang werden
 *    die Felder geleert, auch nach einem Fehler (A19 im Geist — das
 *    Backend kann Felder dieser Oberfläche nicht überschreiben, also
 *    verkürzt die Oberfläche selbst ihre Lebensdauer).
 */
export function MasterPasswordSettings() {
  const { t } = useTranslation();
  const [mode, setMode] = useState<Mode | null>(null);
  const [current, setCurrent] = useState("");
  /** Eigener Zustand für den Rückweg auf den Schlüsselbund — **nicht**
   * derselbe wie beim Passwortwechsel. Zwei Formulare, die sich ein Feld
   * teilen, füllen sich gegenseitig: Wer sein Passwort zum Ändern eintippt
   * und es sich anders überlegt, hätte den Wechselknopf bereits scharf. */
  const [switchCurrent, setSwitchCurrent] = useState("");
  const [password, setPassword] = useState("");
  const [repeated, setRepeated] = useState("");
  const [acknowledged, setAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  /** Klarstellung 10b: Die Frage ist offen, der Wechsel wartet auf sie. */
  const [askAboutAnotherKey, setAskAboutAnotherKey] = useState(false);
  const cancelOverwriteRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    getMasterPasswordMode()
      .then(setMode)
      .catch((err: unknown) => setError(messageFor(t, err)));
  }, [t]);

  const clearSecrets = () => {
    setCurrent("");
    setSwitchCurrent("");
    setPassword("");
    setRepeated("");
    setAcknowledged(false);
  };

  async function run(action: () => Promise<unknown>, onDone: () => void) {
    setBusy(true);
    setError(null);
    setDone(null);
    try {
      await action();
      onDone();
      setMode(await getMasterPasswordMode());
    } catch (err) {
      setError(messageFor(t, err));
      throw err;
    } finally {
      setBusy(false);
    }
  }

  const newPasswordIsUsable =
    characterCount(password) >= MINIMUM_PASSWORD_LENGTH && password === repeated && acknowledged;

  const switchToKeychain = (replaceAnotherKey: boolean) => {
    const currentPassword = switchCurrent;
    void run(
      () => switchToOsKeychain(currentPassword, replaceAnotherKey),
      () => {
        setAskAboutAnotherKey(false);
        setDone(t("masterPassword.switchedToKeychain"));
        clearSecrets();
      },
    ).catch((err: unknown) => {
      // Klarstellung 10b: der eine Fehler mit einer Fortsetzung. Das
      // aktuelle Passwort bleibt dafür stehen — ohne es könnte der zweite,
      // bestätigende Aufruf nicht laufen, und der Nutzer müsste es erneut
      // eingeben, nachdem er gerade eine Warnung gelesen hat.
      if (commandErrorCode(err) === KEYCHAIN_HOLDS_ANOTHER_KEY && !replaceAnotherKey) {
        setAskAboutAnotherKey(true);
        setError(null);
        return;
      }
      setAskAboutAnotherKey(false);
      clearSecrets();
    });
  };

  return (
    <section className="space-y-6">
      <div>
        <h4 className="font-heading mb-1 text-sm font-semibold tracking-wide text-slate-200">
          {t("masterPassword.modeHeading")}
        </h4>
        <p className="text-sm text-slate-300">
          {mode === null
            ? t("common.loading")
            : mode === "password"
              ? t("masterPassword.modePassword")
              : t("masterPassword.modeKeychain")}
        </p>
      </div>

      {mode === "keychain" && (
        <form
          className="space-y-3 border-t border-slate-700 pt-4"
          onSubmit={(e) => {
            e.preventDefault();
            const chosen = password;
            const confirmation = repeated;
            // Klarstellung 12: Das Häkchen reist mit ans Backend — dort
            // wird es geprüft, nicht hier. Der Knopf bleibt ohne es
            // ohnehin aus (`newPasswordIsUsable`); beides zusammen heißt,
            // dass ein umgangenes Formular nichts einrichtet.
            const warningConfirmed = acknowledged;
            void run(
              () => setUpMasterPassword(chosen, confirmation, warningConfirmed),
              () => {
                setDone(t("masterPassword.setUpDone"));
                clearSecrets();
              },
            ).catch(() => clearSecrets());
          }}
        >
          <h4 className="font-heading text-sm font-semibold tracking-wide text-slate-200">
            {t("masterPassword.setUpHeading")}
          </h4>
          <p className="text-sm text-slate-300">{t("masterPassword.setUpIntro")}</p>
          <PasswordField
            label={t("masterPassword.newPasswordLabel")}
            value={password}
            onChange={setPassword}
          />
          <PasswordField
            label={t("masterPassword.repeatPasswordLabel")}
            value={repeated}
            onChange={setRepeated}
          />
          {password.length > 0 && characterCount(password) < MINIMUM_PASSWORD_LENGTH && (
            <p role="alert" className="text-sm text-amber-300">
              {t("masterPassword.passwordTooShort", { minimum: MINIMUM_PASSWORD_LENGTH })}
            </p>
          )}
          {repeated.length > 0 && password !== repeated && (
            <p role="alert" className="text-sm text-amber-300">{t("masterPassword.passwordsDiffer")}</p>
          )}
          <label className="flex items-start gap-2 text-sm text-slate-200">
            <input
              type="checkbox"
              checked={acknowledged}
              onChange={(e) => setAcknowledged(e.target.checked)}
              className="mt-1"
            />
            {t("masterPassword.noRecoveryWarning")}
          </label>
          <Button type="submit" variant="primary" disabled={busy || !newPasswordIsUsable}>
            {t("masterPassword.setUpButton")}
        </Button>
        </form>
      )}

      {mode === "password" && (
        <>
          <form
            className="space-y-3 border-t border-slate-700 pt-4"
            onSubmit={(e) => {
              e.preventDefault();
              const currentPassword = current;
              const chosen = password;
              const confirmation = repeated;
              void run(
                () => changeMasterPassword(currentPassword, chosen, confirmation),
                () => {
                  setDone(t("masterPassword.changeDone"));
                  clearSecrets();
                },
              ).catch(() => clearSecrets());
            }}
          >
            <h4 className="font-heading text-sm font-semibold tracking-wide text-slate-200">
              {t("masterPassword.changeHeading")}
            </h4>
            <PasswordField
              label={t("masterPassword.currentPasswordLabel")}
              value={current}
              onChange={setCurrent}
            />
            <PasswordField
              label={t("masterPassword.newPasswordLabel")}
              value={password}
              onChange={setPassword}
            />
            <PasswordField
              label={t("masterPassword.repeatPasswordLabel")}
              value={repeated}
              onChange={setRepeated}
            />
            <Button type="submit" variant="primary" disabled={
                busy ||
                current.length === 0 ||
                characterCount(password) < MINIMUM_PASSWORD_LENGTH ||
                password !== repeated
              }>
              {t("masterPassword.changeButton")}
          </Button>
          </form>

          <div className="space-y-3 border-t border-slate-700 pt-4">
            <h4 className="font-heading text-sm font-semibold tracking-wide text-slate-200">
              {t("masterPassword.switchHeading")}
            </h4>
            <p className="text-sm text-slate-300">{t("masterPassword.switchIntro")}</p>
            <PasswordField
              label={t("masterPassword.confirmPasswordLabel")}
              value={switchCurrent}
              onChange={setSwitchCurrent}
            />
            <Button disabled={busy || switchCurrent.length === 0} onClick={() => switchToKeychain(false)}>
              {t("masterPassword.switchButton")}
            </Button>
          </div>
        </>
      )}

      {/* Klarstellung 10b: die Frage vor dem Überschreiben eines fremden
       * Schlüssels. Ohne Bestätigung bleibt alles, wie es war. */}
      {askAboutAnotherKey && (
        <DialogFrame
          layer="app"
          size="sm"
          title={t("masterPassword.anotherKeyTitle")}
          onClose={() => {
            setAskAboutAnotherKey(false);
            clearSecrets();
          }}
          initialFocusRef={cancelOverwriteRef}
          actions={
            <>
              <Button
                ref={cancelOverwriteRef}
                onClick={() => {
                  setAskAboutAnotherKey(false);
                  clearSecrets();
                }}
              >
                {t("common.cancel")}
              </Button>
              <Button variant="danger" disabled={busy} onClick={() => switchToKeychain(true)}>
                {t("masterPassword.anotherKeyConfirm")}
              </Button>
            </>
          }
        >
          <p>{t("masterPassword.anotherKeyText")}</p>
        </DialogFrame>
      )}

      {error && (
        <p
          role="alert"
          className="rounded border border-red-800 bg-red-950 px-3 py-2 text-sm text-red-200"
        >
          {error}
        </p>
      )}
      {done && (
        <p className="rounded border border-emerald-800 bg-emerald-950 px-3 py-2 text-sm text-emerald-200">
          {done}
        </p>
      )}
    </section>
  );
}

function PasswordField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <Field label={label}>
      {(control) => (
        <input
          {...control}
          {...TECHNICAL_INPUT_PROPS}
          type="password"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className={`${FIELD_CONTROL_CLASS} w-full`}
        />
      )}
    </Field>
  );
}

function messageFor(
  t: (key: string, options?: Record<string, unknown>) => string,
  err: unknown,
): string {
  return translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err));
}
