import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode, commandErrorMessage, deleteUnusableServer } from "../api";
import { translateErrorCode } from "../errorCodes";
import type { DeleteUnusableServerResult, UnusableServerDto } from "../types";
import { unusableReasonText } from "../unusableServer";
import { Button } from "./ui";

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
        <h2 className="flex flex-wrap items-center gap-2 break-words text-lg font-medium text-fg">
          {server.name}
          <span className="rounded bg-warning-subtle-hover px-2 py-0.5 text-xs font-normal text-warning-fg-softer">
            {t("unusableServer.badge")}
          </span>
        </h2>
        <p className="break-words text-sm text-fg-subtle">{t("unusableServer.host", { host: server.host })}</p>
      </div>
      <p className="rounded border border-warning-border-strong bg-warning-subtle p-3 text-sm text-warning-fg-softer">
        {unusableReasonText(t, server.reason)}
      </p>
      <p className="text-sm text-fg-muted">{t("unusableServer.explanation")}</p>

      {error && <p className="text-sm text-danger-fg">{error}</p>}

      <div className="border-t border-border pt-4">
        {!secretsLeftBehind && (
          <Button variant="danger" size="sm"
            onClick={handleDeleteClick}
          >
            {t("unusableServer.delete")}
          </Button>
        )}

        {secretsLeftBehind && (
          <div
            className="mt-3 rounded border border-warning-border-strong bg-warning-subtle p-3 text-sm"
            data-testid="secrets-left-behind"
          >
            <p className="mb-2 font-medium text-warning-fg-softer">
              {t("serverForm.deletedWithLeftoverSecretsTitle")}
            </p>
            <p className="mb-2 text-warning-fg-softer">{t("serverForm.deletedWithLeftoverSecretsHint")}</p>
            <ul className="mb-2 space-y-1 font-mono text-xs text-warning-fg-softer">
              {secretsLeftBehind.map((ref) => (
                <li key={ref} className="break-all">{ref}</li>
              ))}
            </ul>
            <Button size="sm"
              onClick={() => {
                setSecretsLeftBehind(null);
                onDeleted();
              }}
            >
              {t("common.close")}
            </Button>
          </div>
        )}

        {preview && (
          <div className="mt-3 rounded border border-danger-border bg-danger-subtle p-3 text-sm">
            <p className="mb-2 font-medium text-danger-fg-softer">{t("serverForm.deleteImpactTitle")}</p>
            <ul className="mb-2 space-y-1 text-danger-fg-softer">
              <li>{t("unusableServer.deleteSecrets")}</li>
              {preview.serversLosingJumpHost.map((s) => (
                <li key={s.id}>{t("serverForm.serverWillLoseJumpHost", { name: s.name })}</li>
              ))}
            </ul>
            <p className="mb-2 text-danger-fg-softer">{t("serverForm.deleteAlwaysRemovesHistory")}</p>
            <div className="flex flex-wrap gap-2">
              <Button size="sm"
                onClick={() => setPreview(null)}
              >
                {t("common.cancel")}
              </Button>
              <Button variant="danger" size="sm"
                onClick={handleConfirmDelete}
                disabled={deleting}
              >
                {deleting ? t("common.deleting") : t("serverForm.confirmDeleteServer")}
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
