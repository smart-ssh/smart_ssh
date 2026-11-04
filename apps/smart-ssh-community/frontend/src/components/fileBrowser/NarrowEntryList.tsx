import { useTranslation } from "react-i18next";
import { formatDateTime } from "../../formatDateTime";
import { formatBytes } from "../../format";
import type { RemoteEntryDto } from "../../types";
import type { OpenEntryMenu } from "./shared";
import { Button } from "../ui";

interface NarrowEntryListProps {
  entries: RemoteEntryDto[];
  openMenu: OpenEntryMenu | null;
  setOpenMenu: (menu: OpenEntryMenu | null) => void;
  setDirMenu: (point: { x: number; y: number } | null) => void;
  load: (path: string) => void;
  renderEntryMenu: (entry: RemoteEntryDto, className: string) => React.ReactNode;
}

/** The narrow-layout entry list (one row per entry with a ⋯ menu button). */
export function NarrowEntryList({
  entries,
  openMenu,
  setOpenMenu,
  setDirMenu,
  load,
  renderEntryMenu,
}: NarrowEntryListProps) {
  const { t, i18n } = useTranslation();
  return (
    <ul className="m-0 list-none p-0 text-xs">
      {entries.map((entry) => {
        const rowMenuOpen = openMenu?.anchor === "row" && openMenu.entry.path === entry.path;
        const details = [
          entry.isDir ? null : formatBytes(entry.size),
          entry.modified ? formatDateTime(entry.modified, i18n.language) : null,
        ]
          .filter((part): part is string => part !== null)
          .join(" · ");
        return (
          // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- mouse-only shortcut to the ⋯ menu, same as the table row
          <li
            key={entry.path}
            data-active={openMenu?.entry.path === entry.path ? "true" : undefined}
            className="group flex min-h-11 items-center border-t border-surface-raised/70 pl-2 hover:bg-border/60 focus-within:bg-border/80 data-[active=true]:bg-border/80"
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setDirMenu(null);
              setOpenMenu({ entry, anchor: "row" });
            }}
          >
            <div className="min-w-0 flex-1 py-1">
              {entry.isDir ? (
                <Button variant="bare"
                  type="button"
                  onClick={() => load(entry.path)}
                  className="block max-w-full truncate rounded text-left text-sm text-accent-fg-soft group-hover:underline focus-ring-flush"
                  title={entry.name}
                >
                  📁 {entry.name}
                </Button>
              ) : (
                <span className="block truncate text-sm text-fg-soft group-hover:underline" title={entry.name}>
                  📄 {entry.name}
                </span>
              )}
              <span className="block truncate text-fg-subtle">{details || "—"}</span>
            </div>
            <Button variant="bare"
              type="button"
              onClick={(e) => {
                // Same reasoning as the wide row button: keep the click away from the
                // document-wide "click outside closes" listener.
                e.stopPropagation();
                setOpenMenu(rowMenuOpen ? null : { entry, anchor: "row" });
              }}
              data-menu-trigger={entry.path}
              aria-label={`${t("fileBrowser.actionsColumn")}: ${entry.name}`}
              aria-haspopup="true"
              aria-expanded={rowMenuOpen}
              className="flex min-h-11 min-w-11 shrink-0 items-center justify-center rounded text-lg text-fg-muted focus-ring-flush"
            >
              ⋯
            </Button>
            {rowMenuOpen && renderEntryMenu(entry, "")}
          </li>
        );
      })}
    </ul>
  );
}
