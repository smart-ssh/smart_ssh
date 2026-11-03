import { useTranslation } from "react-i18next";
import { translateErrorCode } from "../../errorCodes";
import type { ServerDto } from "../../types";
import { Button } from "../ui";
import type { IdentityFileState } from "./formTypes";

interface Props {
  loaded: ServerDto;
  identity: IdentityFileState;
}

/** Spec 0076, C-1/C-2/C-7: converts a saved key-file server to a keychain key. */
export function ConvertToKeychainFieldset({ loaded, identity }: Props) {
  const { t } = useTranslation();
  const {
    convertFacts,
    convertFactsError,
    convertConfirmOpen,
    setConvertConfirmOpen,
    converting,
    convertError,
    setConvertError,
    handleConvertToKeychain,
  } = identity;
  return (
          <fieldset className="rounded border border-border p-3">
            <legend className="px-1 text-sm text-fg-muted">
              {t("serverForm.convertToKeychain.legend")}
            </legend>
            <p className="mb-2 text-xs text-fg-subtle">{t("serverForm.convertToKeychain.hint")}</p>
            {!convertConfirmOpen && (
              <>
                  <Button size="sm"
                    onClick={() => setConvertConfirmOpen(true)}
                    disabled={!convertFacts?.validKey}
                  >
                    {t("serverForm.convertToKeychain.button")}
                  </Button>
                  {!convertFacts?.validKey && (
                    <p className="mt-2 text-xs text-warning-fg">
                      {convertFactsError
                        ? t("serverForm.convertToKeychain.checkFailed")
                        : convertFacts
                          ? translateErrorCode(
                              t,
                              convertFacts.problem?.code,
                              convertFacts.problem?.message ?? t("serverForm.convertToKeychain.checking"),
                            )
                          : t("serverForm.convertToKeychain.checking")}
                    </p>
                  )}
              </>
            )}
            {convertConfirmOpen && (
              <div className="rounded border border-border-strong bg-surface-sunken p-3 text-sm">
                <p className="mb-2 text-fg-soft">{t("serverForm.convertToKeychain.confirmWhatChanges")}</p>
                <p className="mb-2 text-fg-subtle">{t("serverForm.convertToKeychain.confirmWhatStays")}</p>
                <p className="mb-3 font-mono text-xs break-all text-fg-muted">
                  {loaded.identityFilePath}
                </p>
                {convertError && <p className="mb-2 text-xs text-danger-fg">{convertError}</p>}
                <div className="flex gap-2">
                  <Button size="sm"
                    onClick={() => {
                      setConvertConfirmOpen(false);
                      setConvertError(null);
                    }}
                  >
                    {t("common.cancel")}
                  </Button>
                  <Button variant="primary" size="sm"
                    onClick={handleConvertToKeychain}
                    disabled={converting}
                  >
                    {converting
                      ? t("serverForm.convertToKeychain.converting")
                      : t("serverForm.convertToKeychain.confirm")}
                  </Button>
                </div>
              </div>
            )}
          </fieldset>
  );
}
