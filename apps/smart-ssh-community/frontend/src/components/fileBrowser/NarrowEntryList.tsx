import { EntryIcon, entryTitle, isBrokenLink, LinkMarker } from "./LinkMarker";
import { useTranslation } from "react-i18next";
import { formatIsoDate } from "../../lib/formatDateTime";
import { formatBytes } from "../../lib/format";
import type { RemoteEntryDto } from "../../api/types";
import type { OpenEntryMenu } from "./shared";
import { Button, Icon } from "../ui";

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
  const { t } = useTranslation();
  return (
    <ul className="m-0 list-none p-0 text-xs" data-testid="narrow-entry-list">
      {entries.map((entry) => {
        const rowMenuOpen = openMenu?.anchor === "row" && openMenu.entry.path === entry.path;
        const details = [
          entry.permissions || null,
          entry.isDir ? (entry.owner ?? null) : formatBytes(entry.size),
          entry.modified ? formatIsoDate(entry.modified) : null,
        ]
          .filter((part): part is string => part !== null)
          .join(" · ");
        return (
          // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- mouse-only shortcut to the ⋯ menu, same as the table row
          <li
            key={entry.path}
            data-active={openMenu?.entry.path === entry.path ? "true" : undefined}
            className="group flex min-h-14 items-center border-t border-border pl-3 hover:bg-border/60 focus-within:bg-border/80 data-[active=true]:bg-border/80"
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
                  className="block max-w-full truncate rounded text-left text-[15px] text-fg-soft group-hover:underline focus-ring-flush"
                  title={entryTitle(entry)}
                >
                  <EntryIcon entry={entry} size={20} /> {entry.name}
                  <LinkMarker entry={entry} />
                </Button>
              ) : (
                <span className={`block truncate text-[15px] text-fg-soft group-hover:underline${isBrokenLink(entry) ? " opacity-50 italic" : ""}`} title={entryTitle(entry)}>
                  <EntryIcon entry={entry} size={20} /> {entry.name}
                  <LinkMarker entry={entry} />
                </span>
              )}
              <span className="block truncate font-mono text-[11px] text-fg-subtle" data-testid="narrow-entry-meta">{details || "—"}</span>
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
              className="flex h-11 w-11 shrink-0 items-center justify-center text-fg-muted hover:text-fg focus-ring-flush"
            >
              <Icon name="more" />
            </Button>
            {rowMenuOpen && renderEntryMenu(entry, "")}
          </li>
        );
      })}
    </ul>
  );
}
