import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { dismissElevationNotice, loadElevationNoticeDismissed } from "../../settings/elevationNoticeSettings";
import { Button } from "../ui";

interface ElevationNoticeProps {
  /** `undefined`: the server is unknown, nothing is shown or stored. */
  serverId: string | undefined;
  elevatedUser: string;
  serverLogging: boolean;
  /** The sudo rule works without a password. */
  passwordless: boolean;
}

/**
 * Issue #1195, Spec 0067 A5: one-time explanation inside the banner on the
 * first switch to elevated rights on a server. It never blocks elevating;
 * "Got it" hides it for this activation (the component is mounted once per
 * activation), "Don't show again" stores the server id.
 */
export function ElevationNotice({ serverId, elevatedUser, serverLogging, passwordless }: ElevationNoticeProps) {
  const { t } = useTranslation();
  const panelId = useId();
  // `null` until the stored list is read, so a dismissed notice never flashes.
  const [visible, setVisible] = useState<boolean | null>(null);
  const [expanded, setExpanded] = useState(true);

  useEffect(() => {
    let cancelled = false;
    if (!serverId) return;
    loadElevationNoticeDismissed()
      .then((ids) => {
        if (!cancelled) setVisible(!ids.includes(serverId));
      })
      .catch((err) => {
        console.warn("Could not read the elevation notice preference:", err);
        if (!cancelled) setVisible(true);
      });
    return () => {
      cancelled = true;
    };
  }, [serverId]);

  if (!serverId || !visible) return null;

  const dontShowAgain = () => {
    setVisible(false);
    dismissElevationNotice(serverId).catch((err) =>
      console.warn("Could not store the elevation notice preference:", err),
    );
  };

  return (
    <div className="border-b border-warning-strong bg-surface-raised px-2 py-1.5 text-xs text-fg">
      <Button
        variant="bare"
        type="button"
        aria-expanded={expanded}
        aria-controls={panelId}
        onClick={() => setExpanded((e) => !e)}
        className="font-semibold focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-warning-strong"
      >
        {expanded ? "▾" : "▸"} {t("fileElevation.notice.title")}
      </Button>
      <output id={panelId} hidden={!expanded} className="mt-1 block space-y-1">
        <p>{t("fileElevation.notice.intro", { user: elevatedUser })}</p>
        <p>{t(serverLogging ? "fileElevation.notice.loggedAppAndServer" : "fileElevation.notice.loggedApp")}</p>
        {passwordless && <p>{t("fileElevation.notice.passwordless", { user: elevatedUser })}</p>}
        <p>{t("fileElevation.notice.alternative")}</p>
        <div className="flex gap-2 pt-1">
          <Button size="sm" type="button" onClick={() => setVisible(false)}>
            {t("fileElevation.notice.gotIt")}
          </Button>
          <Button size="sm" type="button" onClick={dontShowAgain}>
            {t("fileElevation.notice.dontShowAgain")}
          </Button>
        </div>
      </output>
    </div>
  );
}
