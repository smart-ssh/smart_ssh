import { useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

interface FirstRunNoticeScreenProps {
  onAcknowledge: () => void;
}

/**
 * Spec 0031: Zustimmungs-Screen vor der ersten Server-Verbindung
 * (Verantwortung für bestätigte Kommandos + Hinweis auf fehlende
 * zusätzliche Datenbank-Verschlüsselung, Abschnitt 3). "Weiter" bleibt
 * deaktiviert, bis die Checkbox aktiv ist (Abschnitt 4) — reines
 * Wegklicken ohne bewusste Bestätigung ist nicht möglich, deshalb auch
 * kein Abbrechen-/Schließen-Button.
 */
export function FirstRunNoticeScreen({ onAcknowledge }: FirstRunNoticeScreenProps) {
  const { t } = useTranslation();
  const [checked, setChecked] = useState(false);

  // Unabhängiger Review-Pass: s. identischer Kommentar in
  // `HostKeyDialog.tsx` — dieser Screen kann ebenfalls von `ServerList`
  // ausgelöst werden, während `MainScreen` per `display:none` ausgeblendet
  // ist (aktiver Session-Tab), und wäre ohne Portal unsichtbar.
  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <div className="w-full max-w-md rounded-lg bg-slate-800 p-6 shadow-xl">
        <h2 className="font-heading mb-4 text-lg font-semibold tracking-wide text-slate-100">
          {t("firstRunNotice.title")}
        </h2>
        <p className="mb-4 text-sm text-slate-300">{t("firstRunNotice.responsibilityText")}</p>
        <p className="mb-4 text-sm text-slate-300">{t("firstRunNotice.encryptionText")}</p>
        <label className="mb-4 flex items-center gap-2 text-sm text-slate-200">
          <input
            type="checkbox"
            checked={checked}
            onChange={(e) => setChecked(e.target.checked)}
          />
          {t("firstRunNotice.checkboxLabel")}
        </label>
        <div className="flex justify-end">
          <button
            type="button"
            onClick={onAcknowledge}
            disabled={!checked}
            className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t("firstRunNotice.continueButton")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
