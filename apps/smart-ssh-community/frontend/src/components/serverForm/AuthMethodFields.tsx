import { useTranslation } from "react-i18next";
import { pickAndReadTextFile, pickFilePath } from "../../api/fileDialog";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";
import { detectFallbackPlatform, platformCapabilities } from "../../lib/platform";
import { Button } from "../ui";
import { Field, FIELD_CONTROL_CLASS } from "../ui/Field";
import type { IdentityFileState, ServerModel } from "./formTypes";
import { IdentityFileFacts } from "./IdentityFileFacts";
import { authKindLabels, authStateFromKind, type AuthFormState } from "./serverFormModel";

interface Props {
  isCreate: boolean;
  model: ServerModel;
  identity: IdentityFileState;
}

/** The authentication fieldset: method select plus the fields of the method. */
export function AuthMethodFields({ isCreate, model, identity }: Props) {
  const { t } = useTranslation();
  const AUTH_KIND_LABELS = authKindLabels(t);
  const { auth, setAuth, loaded } = model;
  const { identityFacts, identityFactsLoading, identityFactsError } = identity;
  return (
        <fieldset className="rounded border border-border p-3">
          <legend className="px-1 text-sm text-fg-muted">{t("serverForm.authFieldset")}</legend>
          <select
            aria-label={t("serverForm.authFieldset")}
            value={auth.kind}
            onChange={(e) => setAuth(authStateFromKind(e.target.value as AuthFormState["kind"]))}
            className="mb-3 w-full rounded border border-border-strong bg-surface px-2 py-1.5 text-fg"
          >
            <option value="password">{AUTH_KIND_LABELS.password}</option>
            <option value="privateKey">{AUTH_KIND_LABELS.privateKey}</option>
            <option value="agent">{AUTH_KIND_LABELS.agent}</option>
            <option value="certificate">{AUTH_KIND_LABELS.certificate}</option>
            {/* Issue #679: a file path cannot be kept on Android (the picker
             * returns a content URI), so "Key file" is only offered for a
             * server that already uses it; new keys go in via "Private key",
             * which imports the picked content. */}
            {(platformCapabilities(detectFallbackPlatform()).keyFilePath ||
              auth.kind === "identityFile") && (
              <option value="identityFile">{AUTH_KIND_LABELS.identityFile}</option>
            )}
          </select>

          {auth.kind === "password" && (
            <div className="space-y-2">
              <Field
                label={
                  <>
                    {t("serverForm.password")}{" "}
                    <span className="text-fg-subtle">
                      {!isCreate && auth.saved
                        ? t("serverForm.unchangedHint")
                        : t("serverForm.passwordOptionalHint")}
                    </span>
                  </>
                }
              >
                {(control) => (
                  <input
                    {...control}
                    {...TECHNICAL_INPUT_PROPS}
                    type="password"
                    disabled={auth.remove}
                    value={auth.value}
                    onChange={(e) => setAuth({ ...auth, value: e.target.value })}
                    className={`${FIELD_CONTROL_CLASS} w-full`}
                  />
                )}
              </Field>
              {!isCreate && auth.saved && (
                <label className="flex items-center gap-2 text-sm text-fg-muted">
                  <input
                    type="checkbox"
                    checked={auth.remove}
                    onChange={(e) => setAuth({ ...auth, remove: e.target.checked, value: "" })}
                  />
                  {t("serverForm.removeSavedPassword")}
                </label>
              )}
              {!isCreate && !auth.saved && loaded?.authKind === "no_password" && (
                <p className="text-xs text-fg-subtle">{t("serverForm.noPasswordSaved")}</p>
              )}
            </div>
          )}

          {auth.kind === "privateKey" && (
            <div className="space-y-2">
              <div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="text-sm text-fg-muted">
                    {t("serverForm.privateKeyLabel")}{" "}
                    {!isCreate && <span className="text-fg-subtle">{t("serverForm.unchangedHint")}</span>}
                  </span>
                  <Button size="sm"
                    onClick={async () => {
                      const content = await pickAndReadTextFile(
                        t("serverForm.choosePrivateKeyDialogTitle"),
                      );
                      if (content !== null) setAuth({ ...auth, keyContent: content });
                    }}
                  >
                    {t("serverForm.chooseFile")}
                  </Button>
                </div>
                <textarea
                  {...TECHNICAL_INPUT_PROPS}
                  required={isCreate}
                  value={auth.keyContent}
                  onChange={(e) => setAuth({ ...auth, keyContent: e.target.value })}
                  rows={4}
                  className="mt-1 w-full rounded border border-border-strong bg-surface px-2 py-1.5 font-mono text-xs text-fg"
                />
                {/* Spec 0069, Teil C2 (BL-0110): reiner Hinweistext — keine
                 * Prüfung des Inhalts, keine Sperre, kein Einfluss auf
                 * Speichern/Verbinden. RSA-Schlüssel funktionieren
                 * unverändert weiter. */}
                <p className="mt-1 text-xs text-fg-subtle">{t("serverForm.ed25519Recommendation")}</p>
              </div>
              <Field label={t("serverForm.passphraseOptional")}>
                {(control) => (
                  <input
                    {...control}
                    {...TECHNICAL_INPUT_PROPS}
                    type="password"
                    value={auth.passphrase}
                    onChange={(e) => setAuth({ ...auth, passphrase: e.target.value })}
                    className={`${FIELD_CONTROL_CLASS} w-full`}
                  />
                )}
              </Field>
            </div>
          )}

          {auth.kind === "agent" && (
            <p className="text-sm text-fg-subtle">{t("serverForm.agentHint")}</p>
          )}

          {auth.kind === "certificate" && (
            <div className="space-y-2">
              <div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="text-sm text-fg-muted">
                    {t("serverForm.certificateLabel")}{" "}
                    {!isCreate && <span className="text-fg-subtle">{t("serverForm.unchangedHint")}</span>}
                  </span>
                  <Button size="sm"
                    onClick={async () => {
                      const content = await pickAndReadTextFile(
                        t("serverForm.chooseCertificateDialogTitle"),
                      );
                      if (content !== null) setAuth({ ...auth, certContent: content });
                    }}
                  >
                    {t("serverForm.chooseFile")}
                  </Button>
                </div>
                <textarea
                  {...TECHNICAL_INPUT_PROPS}
                  required={isCreate}
                  value={auth.certContent}
                  onChange={(e) => setAuth({ ...auth, certContent: e.target.value })}
                  rows={3}
                  className="mt-1 w-full rounded border border-border-strong bg-surface px-2 py-1.5 font-mono text-xs text-fg"
                />
              </div>
              <div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="text-sm text-fg-muted">
                    {t("serverForm.associatedKey")}{" "}
                    {!isCreate && <span className="text-fg-subtle">{t("serverForm.unchangedHint")}</span>}
                  </span>
                  <Button size="sm"
                    onClick={async () => {
                      const content = await pickAndReadTextFile(t("serverForm.chooseKeyDialogTitle"));
                      if (content !== null) setAuth({ ...auth, keyContent: content });
                    }}
                  >
                    {t("serverForm.chooseFile")}
                  </Button>
                </div>
                <textarea
                  {...TECHNICAL_INPUT_PROPS}
                  required={isCreate}
                  value={auth.keyContent}
                  onChange={(e) => setAuth({ ...auth, keyContent: e.target.value })}
                  rows={3}
                  className="mt-1 w-full rounded border border-border-strong bg-surface px-2 py-1.5 font-mono text-xs text-fg"
                />
              </div>
            </div>
          )}

          {auth.kind === "identityFile" && (
            <div className="space-y-2">
              <div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="text-sm text-fg-muted">{t("serverForm.identityFilePathLabel")}</span>
                  <Button size="sm"
                    onClick={async () => {
                      const path = await pickFilePath(t("serverForm.chooseIdentityFileDialogTitle"));
                      if (path !== null) setAuth({ ...auth, path });
                    }}
                  >
                    {t("serverForm.chooseFile")}
                  </Button>
                </div>
                <input
                  {...TECHNICAL_INPUT_PROPS}
                  type="text"
                  required
                  value={auth.path}
                  onChange={(e) => setAuth({ ...auth, path: e.target.value })}
                  placeholder={t("serverForm.identityFilePathPlaceholder")}
                  className="mt-1 w-full rounded border border-border-strong bg-surface px-2 py-1.5 font-mono text-xs text-fg"
                />
                {/* Spec 0076, B-3: Vorab-Befund — hindert das Speichern
                 * nicht, die Datei darf erst später entstehen. */}
                <IdentityFileFacts
                  loading={identityFactsLoading}
                  facts={identityFacts}
                  error={identityFactsError}
                  emptyPath={auth.path.trim() === ""}
                  path={auth.path}
                />
              </div>
              <Field label={t("serverForm.passphraseOptional")}>
                {(control) => (
                  <input
                    {...control}
                    {...TECHNICAL_INPUT_PROPS}
                    type="password"
                    value={auth.passphrase}
                    onChange={(e) => setAuth({ ...auth, passphrase: e.target.value })}
                    className={`${FIELD_CONTROL_CLASS} w-full`}
                  />
                )}
              </Field>
            </div>
          )}
        </fieldset>
  );
}
