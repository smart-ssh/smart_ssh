import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode, commandErrorMessage, deleteUnusableServer } from "../api";
import { translateErrorCode } from "../errorCodes";
import type { DeleteUnusableServerResult, UnusableServerDto } from "../types";
import { unusableReasonText } from "../unusableServer";

interface UnusableServerPanelProps {
  server: UnusableServerDto;
  onDeleted: () => void;
}

/** Issue #100: Hauptbereich der Verwalten-Ansicht für einen Server, dessen
 * Anmeldeart diese Version nicht lesen kann. Kein Formular — er lässt sich
 * weder bearbeiten noch verbinden. Der einzige Weg ist das Löschen,
 * zweistufig wie im Server-Formular (Vorschau, dann Bestätigung), mit
 * derselben Meldung, wenn ein Secret nicht entfernt werden konnte. */
export function UnusableServerPanel({ server, onDeleted }: UnusableServerPanelProps) {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<DeleteUnusableServerResult | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [secretsLeftBehind, setSecretsLeftBehind] = useState<string[] | null>(null);

  const describe = (err: unknown) =>
    translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err));

  const handleDeleteClick = async () => {
    setError(null);
    try {
      setPreview(await deleteUnusableServer(server.id, false));
    } catch (err) {
      setError(describe(err));
    }
  };

  const handleConfirmDelete = async () => {
    setDeleting(true);
    setError(null);
    try {
      const result = await deleteUnusableServer(server.id, true);
      if (result.secretsLeftBehind.length > 0) {
        setPreview(null);
        setSecretsLeftBehind(result.secretsLeftBehind);
        return;
      }
      onDeleted();
    } catch (err) {
      setError(describe(err));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="space-y-4 p-4" data-testid="unusable-server-panel">
      <div>
        <h2 className="flex items-center gap-2 text-lg font-medium text-slate-100">
          {server.name}
          <span className="rounded bg-amber-900 px-2 py-0.5 text-xs font-normal text-amber-200">
            {t("unusableServer.badge")}
          </span>
        </h2>
        <p className="text-sm text-slate-400">{t("unusableServer.host", { host: server.host })}</p>
      </div>
      <p className="rounded border border-amber-700 bg-amber-950 p-3 text-sm text-amber-200">
        {unusableReasonText(t, server.reason)}
      </p>
      <p className="text-sm text-slate-300">{t("unusableServer.explanation")}</p>

      {error && <p className="text-sm text-red-400">{error}</p>}

      <div className="border-t border-slate-700 pt-4">
        {!secretsLeftBehind && (
          <button
            type="button"
            onClick={handleDeleteClick}
            className="rounded bg-red-900 px-3 py-1.5 text-sm text-red-200 hover:bg-red-800"
          >
            {t("unusableServer.delete")}
          </button>
        )}

        {secretsLeftBehind && (
          <div
            className="mt-3 rounded border border-amber-700 bg-amber-950 p-3 text-sm"
            data-testid="secrets-left-behind"
          >
            <p className="mb-2 font-medium text-amber-200">
              {t("serverForm.deletedWithLeftoverSecretsTitle")}
            </p>
            <p className="mb-2 text-amber-200">{t("serverForm.deletedWithLeftoverSecretsHint")}</p>
            <ul className="mb-2 space-y-1 font-mono text-xs text-amber-200">
              {secretsLeftBehind.map((ref) => (
                <li key={ref}>{ref}</li>
              ))}
            </ul>
            <button
              type="button"
              onClick={() => {
                setSecretsLeftBehind(null);
                onDeleted();
              }}
              className="rounded bg-slate-800 px-2 py-1 text-xs text-slate-200 hover:bg-slate-700"
            >
              {t("common.close")}
            </button>
          </div>
        )}

        {preview && (
          <div className="mt-3 rounded border border-red-800 bg-red-950 p-3 text-sm">
            <p className="mb-2 font-medium text-red-200">{t("serverForm.deleteImpactTitle")}</p>
            <ul className="mb-2 space-y-1 text-red-200">
              <li>{t("unusableServer.deleteSecrets")}</li>
              {preview.serversLosingJumpHost.map((s) => (
                <li key={s.id}>{t("serverForm.serverWillLoseJumpHost", { name: s.name })}</li>
              ))}
            </ul>
            <p className="mb-2 text-red-200">{t("serverForm.deleteAlwaysRemovesHistory")}</p>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => setPreview(null)}
                className="rounded bg-slate-700 px-3 py-1 text-xs hover:bg-slate-600"
              >
                {t("common.cancel")}
              </button>
              <button
                type="button"
                onClick={handleConfirmDelete}
                disabled={deleting}
                className="rounded bg-red-700 px-3 py-1 text-xs text-white hover:bg-red-600 disabled:opacity-50"
              >
                {deleting ? t("common.deleting") : t("serverForm.confirmDeleteServer")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
