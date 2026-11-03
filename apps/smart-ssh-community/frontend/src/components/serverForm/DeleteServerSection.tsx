import { useTranslation } from "react-i18next";
import { Button } from "../ui";
import type { ServerDeleteState } from "./formTypes";
import { authKindLabels } from "./serverFormModel";

interface Props {
  del: ServerDeleteState;
  onDeleted: () => void;
}

/** Delete button with the impact preview and the leftover-secrets notice. */
export function DeleteServerSection({ del, onDeleted }: Props) {
  const { t } = useTranslation();
  const { deleting, deletePreview, secretsLeftBehind, setDeletePreview, setSecretsLeftBehind, handleDeleteClick, handleConfirmDelete } = del;
  return (
            <div className="border-t border-border pt-4">
              <Button variant="danger" size="sm"
                onClick={handleDeleteClick}
              >
                {t("serverForm.deleteServer")}
              </Button>

              {/* Spec 0071, A17: Der Server ist weg, aber mindestens ein
               * Secret blieb im Schlüsselbund. Kein stilles Schließen — der
               * Nutzer braucht die Refs, um die verwaisten Einträge im
               * Schlüsselbund-Verwaltungsprogramm wiederzufinden. */}
              {secretsLeftBehind && (
                <div
                  className="mt-3 rounded border border-warning-border-strong bg-warning-subtle p-3 text-sm"
                  data-testid="secrets-left-behind"
                >
                  <p className="mb-2 font-medium text-warning-fg-softer">
                    {t("serverForm.deletedWithLeftoverSecretsTitle")}
                  </p>
                  <p className="mb-2 text-warning-fg-softer">
                    {t("serverForm.deletedWithLeftoverSecretsHint")}
                  </p>
                  <ul className="mb-2 space-y-1 font-mono text-xs text-warning-fg-softer">
                    {secretsLeftBehind.map((ref) => (
                      <li key={ref}>{ref}</li>
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

              {deletePreview && (
                <div className="mt-3 rounded border border-danger-border bg-danger-subtle p-3 text-sm">
                  <p className="mb-2 font-medium text-danger-fg-softer">{t("serverForm.deleteImpactTitle")}</p>
                  {(deletePreview.server.authKind === "agent" ||
                    deletePreview.server.authKind === "no_password") &&
                  !deletePreview.server.hasSudoPassword &&
                  !deletePreview.server.sudoPasswordUnknown &&
                  deletePreview.serversLosingJumpHost.length === 0 ? (
                    <p className="mb-2 text-danger-fg-softer">{t("serverForm.deleteNoKeychainImpact")}</p>
                  ) : (
                    <ul className="mb-2 space-y-1 text-danger-fg-softer">
                      {/* Spec 0076: `identity_file` hat keinen Secret-Inhalt
                       * im Schlüsselbund, der zwingend existiert — anders als
                       * bei den übrigen Anmeldearten steht dort höchstens
                       * eine OPTIONALE Passphrase, und das DTO sagt nicht,
                       * ob sie gesetzt ist. "Wird gelöscht" wäre hier eine
                       * Behauptung, die die Oberfläche nicht belegen kann;
                       * die eigene "mag gelöscht werden"-Zeile unten (wie bei
                       * `sudoPasswordUnknown`) bleibt deshalb der einzige
                       * Hinweis für diese Anmeldeart. */}
                      {deletePreview.server.authKind !== "agent" &&
                        deletePreview.server.authKind !== "no_password" &&
                        deletePreview.server.authKind !== "identity_file" && (
                          <li>
                            {t("serverForm.secretWillBeDeleted", {
                              label: authKindLabels(t)[deletePreview.server.authKind],
                            })}
                          </li>
                        )}
                      {deletePreview.server.authKind === "identity_file" && (
                        <li>{t("serverForm.identityFilePassphraseMayBeDeleted")}</li>
                      )}
                      {deletePreview.server.hasSudoPassword && (
                        <li>
                          {t("serverForm.secretWillBeDeleted", { label: t("serverForm.sudoLabel") })}
                        </li>
                      )}
                      {/* Spec 0071, A14: Ist der Schlüsselbund nicht lesbar,
                       * darf hier weder "wird gelöscht" noch gar nichts
                       * stehen — beides wäre eine Behauptung über einen
                       * Zustand, den die Oberfläche nicht kennt. */}
                      {deletePreview.server.sudoPasswordUnknown && (
                        <li>
                          {t("serverForm.secretMayBeDeleted", { label: t("serverForm.sudoLabel") })}
                        </li>
                      )}
                      {deletePreview.serversLosingJumpHost.map((s) => (
                        <li key={s.id}>{t("serverForm.serverWillLoseJumpHost", { name: s.name })}</li>
                      ))}
                    </ul>
                  )}
                  {/* spec-reviewer-Fund (Review dieses Schritts): die Vorschau
                   * oben deckt nur Secrets/Jump-Host ab (Spec 0046, Fund 1) —
                   * ohne diesen Hinweis läse sich "keine Secrets, keine
                   * anderen Server betroffen" wie "löschen ist folgenlos",
                   * obwohl Chat-Historie/Notizen/Tags dieses Servers beim
                   * Löschen unwiderruflich mitverschwinden (`ON DELETE
                   * CASCADE`, Spec 0034/0008). */}
                  <p className="mb-2 text-danger-fg-softer">{t("serverForm.deleteAlwaysRemovesHistory")}</p>
                  <div className="flex gap-2">
                    <Button size="sm"
                      onClick={() => setDeletePreview(null)}
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
  );
}
