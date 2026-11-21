import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";
import type { ServerDto } from "../../api/types";
import { ConnectStepLog } from "../ConnectStepLog";
import { Button, TabPanel } from "../ui";
import { Field, FIELD_CONTROL_CLASS } from "../ui/Field";
import { AuthMethodFields } from "./AuthMethodFields";
import { ConvertToKeychainFieldset } from "./ConvertToKeychainFieldset";
import type { ConnectionTestState, IdentityFileState, ServerModel } from "./formTypes";
import { TestResultBadge } from "./TestResultBadge";

/** Technical values: mono, 34 px (44 px narrow). */
const MONO_INPUT = "w-full h-[34px] font-mono narrow:h-11";

interface Props {
  idBase: string;
  active: boolean;
  isCreate: boolean;
  model: ServerModel;
  identity: IdentityFileState;
  test: ConnectionTestState;
  possibleJumpHosts: ServerDto[];
  startDirectoryError: string | null;
  onClearSudoPassword: () => void;
  /** The key-import prompt of Issue #667 (null while closed). */
  importDialog: ReactNode;
}

/** Connection tab: address, jump host, authentication, sudo password,
 * start directory, advanced options and the connection test. */
export function ConnectionTab({
  idBase,
  active,
  isCreate,
  model,
  identity,
  test,
  possibleJumpHosts,
  startDirectoryError,
  onClearSudoPassword: handleClearSudoPassword,
  importDialog,
}: Props) {
  const { t } = useTranslation();
  const {
    loaded,
    host,
    setHost,
    port,
    setPort,
    username,
    setUsername,
    jumpHost,
    setJumpHost,
    sudoPassword,
    setSudoPassword,
    hasSudoPassword,
    sudoPasswordUnknown,
    clearingSudoPassword,
    startDirectory,
    setStartDirectory,
    sftpServerPath,
    setSftpServerPath,
    elevationAllowed,
    setElevationAllowed,
  } = model;
  const { testing, testResult, runTest } = test;
  return (
    <TabPanel idBase={idBase} id="connection" active={active}>
      <div className="grid grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)] gap-x-6 gap-y-4 narrow:grid-cols-1">
        <div className="min-w-0 space-y-3">
          <div className="grid grid-cols-[minmax(0,1fr)_90px] gap-3">
            <Field label={t("serverForm.host")}>
              {(control) => (
                <input
                  {...control}
                  {...TECHNICAL_INPUT_PROPS}
                  type="text"
                  required
                  value={host}
                  onChange={(e) => setHost(e.target.value)}
                  className={`${FIELD_CONTROL_CLASS} ${MONO_INPUT}`}
                />
              )}
            </Field>
            <Field label={t("serverForm.port")}>
              {(control) => (
                <input
                  {...control}
                  type="number"
                  required
                  min={1}
                  max={65535}
                  value={port}
                  onChange={(e) => setPort(e.target.value)}
                  className={`${FIELD_CONTROL_CLASS} ${MONO_INPUT}`}
                />
              )}
            </Field>
          </div>
          <Field label={t("serverForm.username")}>
            {(control) => (
              <input
                {...control}
                {...TECHNICAL_INPUT_PROPS}
                type="text"
                required
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                className={`${FIELD_CONTROL_CLASS} ${MONO_INPUT}`}
              />
            )}
          </Field>

          <Field label={t("serverForm.jumpHost")}>
            {(control) => (
              <select
                {...control}
                value={jumpHost ?? ""}
                onChange={(e) => setJumpHost(e.target.value || null)}
                className={`${FIELD_CONTROL_CLASS} w-full narrow:min-h-11`}
              >
                <option value="">{t("serverForm.noJumpHost")}</option>
                {possibleJumpHosts.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
            )}
          </Field>

          <AuthMethodFields idBase={idBase} isCreate={isCreate} model={model} identity={identity} />

          {importDialog}

          {/* Spec 0076, C-1/C-2/C-7 (BL-0222): only for a saved server whose
           * stored auth kind (not just the draft) is a key file — the button acts
           * on the saved server, not on the open draft. */}
          {!isCreate && loaded && loaded.authKind === "identity_file" && (
            <ConvertToKeychainFieldset loaded={loaded} identity={identity} />
          )}

          <div className="flex flex-wrap items-center gap-2">
            <Button onClick={runTest} disabled={testing}>
              {testing ? t("common.testing") : t("serverForm.testConnection")}
            </Button>
            {testResult && <TestResultBadge result={testResult} />}
          </div>
          {/* Issue #51: Schritt-Protokoll des letzten Tests, zugeklappt —
           * bei Erfolg wie bei Fehlschlag. */}
          {testResult?.steps && testResult.steps.length > 0 && (
            <ConnectStepLog steps={testResult.steps} />
          )}
        </div>

        <div className="min-w-0 space-y-4">
          <fieldset className="min-w-0 space-y-2 rounded border border-border p-3">
            <legend className="px-1 text-label">{t("serverForm.sudoFieldset")}</legend>
            <p className="text-xs text-fg-subtle">{t("serverForm.sudoHint")}</p>
            <Field
              label={
                <>
                  {t("serverForm.sudoLabel")}{" "}
                  <span className="text-fg-subtle">
                    {sudoPasswordUnknown
                      ? t("serverForm.sudoUnchangedUnknown")
                      : hasSudoPassword
                        ? t("serverForm.sudoUnchangedStored")
                        : t("serverForm.sudoUnchanged")}
                  </span>
                </>
              }
            >
              {(control) => (
                <input
                  {...control}
                  {...TECHNICAL_INPUT_PROPS}
                  type="password"
                  value={sudoPassword}
                  onChange={(e) => setSudoPassword(e.target.value)}
                  className={`${FIELD_CONTROL_CLASS} ${MONO_INPUT}`}
                />
              )}
            </Field>
            {(hasSudoPassword || sudoPasswordUnknown) && (
              <Button
                variant="danger"
                size="sm"
                onClick={handleClearSudoPassword}
                disabled={clearingSudoPassword}
              >
                {clearingSudoPassword ? t("common.removing") : t("serverForm.removeSudoPassword")}
              </Button>
            )}
          </fieldset>

          {/* "Advanced" is always shown open in the redesign (start
           * directory, elevation, SFTP path). */}
          <fieldset className="min-w-0 space-y-3 rounded border border-border p-3">
            <legend className="px-1 text-label">{t("serverForm.advanced")}</legend>
            <Field
              label={t("serverForm.startDirectoryLabel")}
              help={startDirectoryError ? undefined : t("serverForm.startDirectoryHint")}
              error={startDirectoryError ?? undefined}
            >
              {(control) => (
                <input
                  {...control}
                  {...TECHNICAL_INPUT_PROPS}
                  value={startDirectory}
                  onChange={(e) => setStartDirectory(e.target.value)}
                  placeholder={t("serverForm.startDirectoryPlaceholder")}
                  className={`${FIELD_CONTROL_CLASS} ${MONO_INPUT}`}
                />
              )}
            </Field>
            <div>
              <label className="flex items-start gap-2 text-sm text-fg-muted">
                <input
                  type="checkbox"
                  checked={elevationAllowed}
                  aria-describedby={`${idBase}-elevation-allowed-hint`}
                  onChange={(e) => setElevationAllowed(e.target.checked)}
                  className="mt-1"
                />
                <span>{t("serverForm.elevationAllowedLabel")}</span>
              </label>
              <p id={`${idBase}-elevation-allowed-hint`} className="mt-1 text-xs text-fg-subtle">
                {t("serverForm.elevationAllowedHint")}
              </p>
            </div>
            <Field
              label={t("serverForm.sftpServerPathLabel")}
              help={t("serverForm.sftpServerPathHint")}
            >
              {(control) => (
                <input
                  {...control}
                  {...TECHNICAL_INPUT_PROPS}
                  value={sftpServerPath}
                  disabled={!elevationAllowed}
                  onChange={(e) => setSftpServerPath(e.target.value)}
                  placeholder={t("serverForm.sftpServerPathPlaceholder")}
                  className={`${FIELD_CONTROL_CLASS} ${MONO_INPUT}`}
                />
              )}
            </Field>
          </fieldset>
        </div>
      </div>
    </TabPanel>
  );
}
