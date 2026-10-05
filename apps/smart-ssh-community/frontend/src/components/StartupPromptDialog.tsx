import { useState } from "react";
import { useTranslation } from "react-i18next";
import { characterCount } from "../passwordLength";
import type { StartupPromptAnswer, StartupPromptKind, StartupPromptRequest } from "../types";

/** Spec 0101, A13: Mindestlänge des Master-Passworts.
 *
 * Das Backend prüft dieselbe Grenze und lehnt mit
 * `MASTER_PASSWORD_REJECTED` ab — diese hier ist die freundliche Hälfte
 * (der Knopf bleibt aus, statt den Nutzer erst abschicken zu lassen), nicht
 * die maßgebliche. */
const MINIMUM_PASSWORD_LENGTH = 12;

interface StartupPromptDialogProps {
  request: StartupPromptRequest;
  /** Schickt die Antwort ans Backend. `password`/`repeated` und die
   * Bestätigung der Warnung (A13/E10, Klarstellung 12) nur bei
   * `newMasterPassword`. */
  onAnswer: (
    answer: StartupPromptAnswer,
    password?: string,
    repeated?: string,
    warningConfirmed?: boolean,
  ) => void;
  /** Nur für `notice`: wegklicken, **ohne** eine Antwort zu schicken
   * (Klarstellung 10e — das Backend wartet dort auf keine). */
  onDismiss: () => void;
}

/** Welche Knöpfe eine Frageart zeigen darf.
 *
 * **Die Liste ist eine Positivliste** (A3): Das Frontend bildet die Knöpfe
 * ab, die das Backend je Frageart vorsieht, und erfindet keine. Ein Knopf,
 * der hier nicht steht, erscheint nicht — und das Backend würde eine
 * erfundene Antwort ohnehin in „beenden" übersetzen
 * (`window_prompt::choice_for`). Doppelt, weil die Antwort über IPC geht.
 */
const ANSWERS_BY_KIND: Record<StartupPromptKind, StartupPromptAnswer[]> = {
  retryOrQuit: ["retry", "quit"],
  retrySkipOrQuit: ["retry", "continueWithoutMigration", "quit"],
  retrySetUpOrQuit: ["retry", "setUpMasterPassword", "quit"],
  startOverOrQuit: ["startOver", "quit"],
  newKeyOrQuit: ["generateNewKey", "quit"],
  confirmStartOver: ["confirm", "cancel"],
  confirmNewKey: ["confirm", "cancel"],
  newMasterPassword: ["confirm", "cancel"],
  notice: [],
};

/** Die Wahlen, die Daten aufgeben oder einen neuen Schlüssel erzeugen (A3,
 * A5) — rot, damit sie sich vom Normalfall unterscheiden. */
const DESTRUCTIVE: ReadonlySet<StartupPromptAnswer> = new Set<StartupPromptAnswer>([
  "startOver",
  "generateNewKey",
  "continueWithoutMigration",
]);

/**
 * Spec 0101, Teil 0 Frage 3: die Startdialoge **im Fenster** (D1–D4, die
 * zweiten Bestätigungen aus A5, die Eingabe eines neuen Master-Passworts
 * aus A13 und der Hinweis nach dem Umbenennen).
 *
 * **Titel und Text kommen fertig aus dem Backend** (`app_logic::
 * startup_choice_dialogs`): Sie nennen Dateinamen, die Ursache und den
 * nächsten Schritt, und sie entstehen in der Sprache, die der Start einmal
 * aus der Umgebung bestimmt hat (ADR 0095 §8). Hier stehen nur die
 * Knopfbeschriftungen — sonst gäbe es zwei Quellen für denselben Text.
 *
 * **Ein Hinweis wartet auf keine Antwort** (Klarstellung 10e): Bei `notice`
 * schickt „OK" **nichts** ans Backend; der Startablauf ist dort längst
 * weitergelaufen. Ein `answerStartupPrompt` darauf wäre eine Antwort ohne
 * Frage — und könnte im schlechtesten Fall die nächste, echte Frage
 * vorab beantworten.
 */
export function StartupPromptDialog({
  request,
  onAnswer,
  onDismiss,
}: StartupPromptDialogProps) {
  const { t } = useTranslation();
  // Klarstellung 10e, zweite Hälfte: **ein geöffnetes Passwortfeld ist
  // leer.** Das trägt der Aufrufer mit: Er gibt dem Dialog je Frage einen
  // neuen `key`, also entsteht dieser Zustand je Frage neu. Ein Passwort
  // aus einem früheren, abgebrochenen Versuch kann hier nicht stehen (und
  // das Backend leert seinen Platz zusätzlich selbst).
  const [password, setPassword] = useState("");
  const [repeated, setRepeated] = useState("");
  const [acknowledged, setAcknowledged] = useState(false);

  const wantsPassword = request.kind === "newMasterPassword";
  // Die zweite Bestätigung aus A5 gibt den bisherigen Verlauf auf bzw.
  // erzeugt einen neuen Schlüssel — sie gehört zu den roten Knöpfen, auch
  // wenn die Antwort nur „confirm" heißt.
  const confirmIsDestructive =
    request.kind === "confirmStartOver" || request.kind === "confirmNewKey";
  const passwordIsUsable =
    characterCount(password) >= MINIMUM_PASSWORD_LENGTH && password === repeated && acknowledged;

  const answer = (choice: StartupPromptAnswer) => {
    if (choice === "confirm" && wantsPassword) {
      // Klarstellung 12: Das Häkchen reist mit — geprüft wird es im
      // Backend (`LossWarning`), der ausgegraute Knopf ist nur die
      // freundliche Hälfte.
      onAnswer(choice, password, repeated, acknowledged);
      return;
    }
    onAnswer(choice);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <div className="w-full max-w-md rounded-lg bg-slate-800 p-6 shadow-xl">
        <h2 className="font-heading mb-4 text-lg font-semibold tracking-wide text-slate-100">
          {request.title}
        </h2>
        {/* `whitespace-pre-line`: Die Backend-Texte gliedern Ursache,
         * Datenpfad und nächsten Schritt in Zeilen (Spec 0059). */}
        <p className="mb-4 whitespace-pre-line text-sm text-slate-300">{request.message}</p>

        {wantsPassword && (
          <div className="mb-4 space-y-3">
            <label className="block text-sm text-slate-200">
              {t("startup.newPasswordLabel")}
              <input
                type="password"
                autoFocus
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-3 py-2 text-sm text-slate-100"
              />
            </label>
            <label className="block text-sm text-slate-200">
              {t("startup.repeatPasswordLabel")}
              <input
                type="password"
                value={repeated}
                onChange={(e) => setRepeated(e.target.value)}
                className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-3 py-2 text-sm text-slate-100"
              />
            </label>
            {password.length > 0 && characterCount(password) < MINIMUM_PASSWORD_LENGTH && (
              <p className="text-sm text-amber-300">
                {t("startup.passwordTooShort", { minimum: MINIMUM_PASSWORD_LENGTH })}
              </p>
            )}
            {repeated.length > 0 && password !== repeated && (
              <p className="text-sm text-amber-300">{t("startup.passwordsDiffer")}</p>
            )}
            {/* A13/E10: die Warnung mit ausdrücklicher Bestätigung — ohne
             * Passwort sind die Daten verloren, es gibt keine
             * Wiederherstellung. */}
            <label className="flex items-start gap-2 text-sm text-slate-200">
              <input
                type="checkbox"
                checked={acknowledged}
                onChange={(e) => setAcknowledged(e.target.checked)}
                className="mt-1"
              />
              {t("startup.noRecoveryWarning")}
            </label>
          </div>
        )}

        <div className="flex flex-wrap justify-end gap-2">
          {request.kind === "notice" ? (
            <button
              type="button"
              onClick={onDismiss}
              className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
            >
              {t("common.ok")}
            </button>
          ) : (
            ANSWERS_BY_KIND[request.kind].map((choice) => (
              <button
                key={choice}
                type="button"
                onClick={() => answer(choice)}
                disabled={choice === "confirm" && wantsPassword && !passwordIsUsable}
                className={`rounded px-4 py-2 text-sm font-medium text-white disabled:cursor-not-allowed disabled:opacity-50 ${
                  DESTRUCTIVE.has(choice) || (choice === "confirm" && confirmIsDestructive)
                    ? "bg-red-700 hover:bg-red-600"
                    : choice === "quit" || choice === "cancel"
                      ? "bg-slate-700 hover:bg-slate-600"
                      : "bg-indigo-600 hover:bg-indigo-500"
                }`}
              >
                {t(`startup.answers.${choice}`)}
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
