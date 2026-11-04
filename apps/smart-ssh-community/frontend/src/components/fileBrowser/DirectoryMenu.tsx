import { useTranslation } from "react-i18next";
import { Menu, MenuItem } from "../ui";

/** Issue #570: context menu on the empty area of the file browser — kept as
 * its own small component so `FileEntryMenu` only holds entry actions. */
export function DirectoryMenu({
  narrow,
  x,
  y,
  pasteName,
  clipboardFiles,
  onNewFolder,
  onNewFile,
  onRefresh,
  onPaste,
  onClose,
}: {
  narrow: boolean;
  x: number;
  y: number;
  pasteName: string | null;
  clipboardFiles: boolean;
  onNewFolder: () => void;
  onNewFile: () => void;
  onRefresh: () => void;
  onPaste: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Menu
      label={t("fileBrowser.directoryMenuLabel")}
      anchor={{ x, y }}
      onClose={onClose}
      narrow={narrow}
      className="z-20 w-52"
    >
      <MenuItem narrow={narrow} onClick={onNewFolder}>
        {t("fileBrowser.menu.newFolder")}
      </MenuItem>
      <MenuItem narrow={narrow} onClick={onNewFile}>
        {t("fileBrowser.menu.newFile")}
      </MenuItem>
      <MenuItem narrow={narrow} onClick={onRefresh}>
        {t("fileBrowser.menu.refresh")}
      </MenuItem>
      {pasteName !== null && (
        <MenuItem narrow={narrow} onClick={onPaste} title={t("fileBrowser.pasteTitle", { name: pasteName })}>
          {t("fileBrowser.paste")}
        </MenuItem>
      )}
      {pasteName === null && clipboardFiles && (
        <MenuItem narrow={narrow} onClick={onPaste} title={t("fileBrowser.clipboardPaste.menuTitle")}>
          {t("fileBrowser.paste")}
        </MenuItem>
      )}
    </Menu>
  );
}
