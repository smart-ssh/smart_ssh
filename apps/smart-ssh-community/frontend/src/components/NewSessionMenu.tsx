// Issue #657: context menu with the single entry "New session", shared by
// the server list and the session tabs.
import { useTranslation } from "react-i18next";
import { Menu, MenuItem } from "./ui";

export function NewSessionMenu({
  x,
  y,
  serverName,
  onNewSession,
  onKeepAsRegular,
  onClose,
}: {
  x: number;
  y: number;
  serverName: string;
  /** Omitted for a group: the menu then only offers "Keep as regular entry". */
  onNewSession?: () => void;
  /** Spec 0103: only given for an entry that carries the copy marker. */
  onKeepAsRegular?: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Menu label={serverName} anchor={{ x, y }} onClose={onClose} className="z-30 w-56">
      {onNewSession && <MenuItem onClick={onNewSession}>{t("sessionTabs.newSession")}</MenuItem>}
      {onKeepAsRegular && <MenuItem onClick={onKeepAsRegular}>{t("copyMarker.keep")}</MenuItem>}
    </Menu>
  );
}
