import { useTranslation } from "react-i18next";
import type { useLocalEditSession } from "../../useLocalEditSession";
import { Button } from "../ui";

interface LocalEditStatusBarProps {
  localEdit: ReturnType<typeof useLocalEditSession>;
  editIdentityMismatch: boolean;
  channelUser: string | null;
  /** The platform opens an external editor, so no in-app edit button is shown. */
  externalEditor: boolean;
  onOfferUpload: (asOther?: boolean) => void;
  onOpenInAppEditor: () => void;
}

/** Status bar of the "open locally" flow (Spec 0054, Teil 4). */
export function LocalEditStatusBar({
  localEdit,
  editIdentityMismatch,
  channelUser,
  externalEditor,
  onOfferUpload,
  onOpenInAppEditor,
}: LocalEditStatusBarProps) {
  const { t } = useTranslation();
  if (!localEdit.session) return null;
  return (
      <div className="flex items-center gap-2 border-t border-accent-border-strong/50 bg-accent-subtle/40 px-2 py-1.5 text-xs">
        {localEdit.session.status === "uploading" ? (
          <span className="text-fg-muted">
            {t("fileBrowser.localEdit.uploading", { name: localEdit.session.entry.name })}
          </span>
        ) : localEdit.session.status === "changed" ? (
          <>
            <span className="flex-1 text-warning-fg-soft">
              {t("fileBrowser.localEdit.changed", { name: localEdit.session.entry.name })}
            </span>
            {editIdentityMismatch ? (
              <Button size="sm" variant="primary" onClick={() => onOfferUpload(true)}>
                {channelUser
                  ? t("fileElevation.editUploadAs", { user: channelUser })
                  : t("fileElevation.editUploadAsLogin")}
              </Button>
            ) : (
              <Button size="sm" variant="primary" onClick={() => onOfferUpload()}>
                {t("fileBrowser.upload")}
              </Button>
            )}
            <Button size="sm" onClick={localEdit.dismissChange}>
              {t("fileBrowser.localEdit.later")}
            </Button>
          </>
        ) : (
          <span className="flex-1 text-fg-muted">
            {t("fileBrowser.localEdit.editing", { name: localEdit.session.entry.name })}
            {localEdit.session.elevatedUser && (
              <span className="ml-1 text-warning-fg-soft">
                {t("fileElevation.editOpenedAs", { user: localEdit.session.elevatedUser })}
              </span>
            )}
          </span>
        )}
        {!externalEditor && localEdit.session.status === "editing" && (
          <Button size="sm" onClick={() => onOpenInAppEditor()}>
            {t("common.edit")}
          </Button>
        )}
        <Button size="sm" onClick={localEdit.endSession}>
          {t("fileBrowser.localEdit.end")}
        </Button>
        {editIdentityMismatch && (
          <span className="w-full text-warning-fg-soft">
            {localEdit.session.elevatedUser
              ? channelUser
                ? t("fileElevation.editIdentityChanged", {
                    from: localEdit.session.elevatedUser,
                    user: channelUser,
                  })
                : t("fileElevation.editIdentityElevatedOff", {
                    from: localEdit.session.elevatedUser,
                  })
              : t("fileElevation.editIdentityElevatedOn", { user: channelUser })}
          </span>
        )}
        {localEdit.session.error && (
          <span className="w-full text-danger-fg-soft">{localEdit.session.error}</span>
        )}
      </div>
  );
}
