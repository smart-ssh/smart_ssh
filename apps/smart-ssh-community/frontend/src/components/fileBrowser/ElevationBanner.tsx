import { useTranslation } from "react-i18next";
import { Button } from "../ui";

interface ElevationBannerProps {
  elevatedUser: string | null;
  warningPath: string | null | undefined;
  onToggleOff: () => void;
}

/** Spec 0067: banner while the elevated mode is on, plus the unsafe-server warning. */
export function ElevationBanner({ elevatedUser, warningPath, onToggleOff }: ElevationBannerProps) {
  const { t } = useTranslation();
  return (
    <>
      <div
        role="alert"
        className="flex items-center gap-2 bg-warning px-2 py-1.5 text-xs font-semibold text-surface-sunken"
      >
        <span className="flex-1">⚠ {t("fileElevation.banner", { user: elevatedUser })}</span>
        <Button variant="bare"
          type="button"
          onClick={onToggleOff}
          className="rounded border border-surface-sunken/40 px-2 py-0.5 hover:bg-warning-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-surface-sunken"
        >
          {t("fileElevation.toggleOff")}
        </Button>
      </div>
      {warningPath && (
        <div role="alert" className="bg-danger-subtle px-2 py-1.5 text-xs text-danger-fg-softer">
          {t("fileElevation.failure.sftpServerUnsafe", { path: warningPath })}
        </div>
      )}
    </>
  );
}
