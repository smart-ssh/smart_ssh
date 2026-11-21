import { useTranslation } from "react-i18next";
import { pickAndReadTextFile, pickFilePath } from "../../api/fileDialog";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";
import { detectFallbackPlatform, platformCapabilities } from "../../lib/platform";
import { Button } from "../ui";
import { Field, FIELD_CONTROL_CLASS } from "../ui/Field";
import type { IdentityFileState, ServerModel } from "./formTypes";
import { AuthKindSegments } from "./AuthKindSegments";
import { IdentityFileFacts } from "./IdentityFileFacts";
import { authKindLabels, authStateFromKind, type AuthFormState } from "./serverFormModel";

interface Props {
  idBase: string;
  isCreate: boolean;
  model: ServerModel;
  identity: IdentityFileState;
}

/** The authentication fieldset: method select plus the fields of the method. */
export function AuthMethodFields({ idBase, isCreate, model, identity }: Props) {
  const { t } = useTranslation();
  const AUTH_KIND_LABELS = authKindLabels(t);
  const { auth, setAuth, loaded } = model;
  const keyFileOffered =
    platformCapabilities(detectFallbackPlatform()).keyFilePath || auth.kind === "identityFile";
  // Issue #679: a file path cannot be kept on Android (the picker returns a
  // content URI), so "Key file" is only offered for a server that already
  // uses it; new keys go in via "Private key", which imports the content.
  const authOptions: { kind: AuthFormState["kind"]; label: string }[] = [
    { kind: "password", label: AUTH_KIND_LABELS.password },
    { kind: "privateKey", label: AUTH_KIND_LABELS.privateKey },
    { kind: "agent", label: AUTH_KIND_LABELS.agent },
    { kind: "certificate", label: AUTH_KIND_LABELS.certificate },
    ...(keyFileOffered
      ? [{ kind: "identityFile" as const, label: AUTH_KIND_LABELS.identityFile }]
      : []),
  ];
  const { identityFacts, identityFactsLoading, identityFactsError } = identity;
  return (
        <fieldset className="min-w-0 space-y-3">
          <legend className="mb-1 text-label">{t("serverForm.authFieldset")}</legend>
          <AuthKindSegments
            value={auth.kind}
            options={authOptions}
            onChange={(kind) => setAuth(authStateFromKind(kind))}
          />

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
                  className={`${FIELD_CONTROL_CLASS} mt-1 w-full font-mono text-xs`}
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
                  className={`${FIELD_CONTROL_CLASS} mt-1 w-full font-mono text-xs`}
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
                  className={`${FIELD_CONTROL_CLASS} mt-1 w-full font-mono text-xs`}
                />
              </div>
            </div>
          )}

          {auth.kind === "identityFile" && (
            <div className="space-y-2">
              <div>
                <label htmlFor={`${idBase}-identity-path`} className="text-label">
                  {t("serverForm.identityFilePathLabel")}
                </label>
                <div className="mt-1 flex items-center gap-2">
                  <input
                    id={`${idBase}-identity-path`}
                    {...TECHNICAL_INPUT_PROPS}
                    type="text"
                    required
                    value={auth.path}
                    onChange={(e) => setAuth({ ...auth, path: e.target.value })}
                    placeholder={t("serverForm.identityFilePathPlaceholder")}
                    className={`${FIELD_CONTROL_CLASS} min-w-0 flex-1 font-mono`}
                  />
                  <Button
                    onClick={async () => {
                      const path = await pickFilePath(t("serverForm.chooseIdentityFileDialogTitle"));
                      if (path !== null) setAuth({ ...auth, path });
                    }}
                  >
                    {t("serverForm.chooseFile")}
                  </Button>
                </div>
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
