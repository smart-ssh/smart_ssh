import type { RefObject } from "react";
import { useTranslation } from "react-i18next";
import { detectFallbackPlatform, platformCapabilities } from "../../lib/platform";
import { Button } from "../ui";
import { DialogFrame } from "../ui/DialogFrame";
import type { IdentityFileState, ServerModel } from "./formTypes";

interface Props {
  model: ServerModel;
  identity: IdentityFileState;
  saving: boolean;
  importPrimaryRef: RefObject<HTMLButtonElement | null>;
  cancelImportPrompt: () => void;
  save: (importKey: boolean) => void;
}

/** Issue #667: shown once when a key-file server is saved with a new or
 * changed path. */
export function ImportKeyDialog({ model, identity, saving, importPrimaryRef, cancelImportPrompt, save }: Props) {
  const { t } = useTranslation();
  const { auth } = model;
  const { importError, importSavedId, finishKeepingPath } = identity;
  return (
          <DialogFrame
            layer="app"
            size="md"
            title={t("serverForm.importKey.title")}
            onClose={cancelImportPrompt}
            initialFocusRef={importPrimaryRef}
            actions={
              <>
                <Button size="sm" disabled={saving} onClick={cancelImportPrompt}>
                  {t("common.cancel")}
                </Button>
                {platformCapabilities(detectFallbackPlatform()).keyFilePath && (
                  <Button
                    size="sm"
                    disabled={saving}
                    onClick={() => (importSavedId !== null ? finishKeepingPath() : save(false))}
                  >
                    {t("serverForm.importKey.keep")}
                  </Button>
                )}
                <Button ref={importPrimaryRef} variant="primary" size="sm" disabled={saving} onClick={() => save(true)}>
                  {t("serverForm.importKey.import")}
                </Button>
              </>
            }
          >
            <p className="mb-2 font-mono text-xs break-all text-fg-muted">{auth.kind === "identityFile" ? auth.path : ""}</p>
            <ul className="mb-3 space-y-1 text-sm text-fg-muted">
              <li>{t("serverForm.importKey.importHint")}</li>
              {platformCapabilities(detectFallbackPlatform()).keyFilePath && (
                <li>{t("serverForm.importKey.keepHint")}</li>
              )}
            </ul>
            {importError && (
              <p role="alert" className="mb-2 text-xs text-danger-fg">
                {importError}
              </p>
            )}
          </DialogFrame>
  );
}
