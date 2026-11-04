import { COLUMN_ORDER } from "./shared";
import { useTranslation } from "react-i18next";
import { type HideableFileColumn } from "../../settings/layoutSettings";
import { Menu, MenuItem } from "../ui";

/** Issue #653: right-click menu on the header row — one checkbox per
 * column except Name, which cannot be hidden. */
export function ColumnsMenu({
  point,
  hidden,
  onToggle,
  onClose,
}: {
  point: { x: number; y: number };
  hidden: readonly HideableFileColumn[];
  onToggle: (column: HideableFileColumn) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Menu label={t("fileBrowser.columnsMenu")} anchor={point} onClose={onClose} className="z-10">
      {COLUMN_ORDER.map((c) => (
        <MenuItem
          key={c}
          checked={!hidden.includes(c)}
          icon={hidden.includes(c) ? "" : "✓"}
          onClick={() => onToggle(c)}
        >
          {t(`fileBrowser.column.${c}`)}
        </MenuItem>
      ))}
    </Menu>
  );
}
