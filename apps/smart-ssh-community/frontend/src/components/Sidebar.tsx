import { useRef, useState } from "react";
import { detectFallbackPlatform, platformCapabilities } from "../titlebarLayout";
import { useTranslation } from "react-i18next";
import { buildGroupTree, type GroupTreeNode } from "../groupTree";
import { folderForNewItem } from "../newItemFolder";
import {
  classifyDrop,
  dropHighlightClass,
  groupDropTargetValue,
  type DragItem,
  type DropTarget,
} from "../treeDrag";
import type { GroupDto, ServerDto, UnusableServerDto } from "../types";
import { useTreeDrag } from "../useTreeDrag";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { MoveToDialog } from "./MoveToDialog";
import { useManageViews } from "../extensions/FormSlots";
import { TreeDragGhost } from "./TreeDragGhost";
import { unusableReasonText } from "../unusableServer";
import { Button } from "./ui";

export type Selection =
  | { kind: "group"; id: string }
  | { kind: "server"; id: string }
  /** Issue #100: ein nicht nutzbarer Server (Anmeldeart unlesbar). */
  | { kind: "unusableServer"; id: string }
  | { kind: "newGroup"; parentId: string | null }
  | { kind: "newServer"; groupId: string | null }
  /** Spec 0038, Abschnitt 4: eine von einer Erweiterung registrierte Ansicht. */
  | { kind: "extensionView"; id: string };

interface SidebarProps {
  groups: GroupDto[];
  servers: ServerDto[];
  /** Issue #100: Server, deren Anmeldeart diese Version nicht lesen kann —
   * im Baum sichtbar und auswählbar (zum Löschen), aber nicht ziehbar. */
  unusableServers?: UnusableServerDto[];
  selection: Selection | null;
  onSelect: (selection: Selection) => void;
  /** Spec 0075, §3.1.7/§3.2 — Import aus bzw. Export nach `ssh_config`. */
  onImportSshConfig: () => void;
  onExportSshConfig: () => void;
  /** Issue #48 / Spec 0103: ein Server oder eine Gruppe wurde per
   * Drag-and-drop auf ein Ziel gezogen, das etwas ändert (`valid`) oder
   * einen Zyklus bilden würde (`cycle`, wird vom Backend abgelehnt).
   * Unverändernde Ablagen (`noop`) kommen hier nie an. */
  onMove: (item: DragItem, target: DropTarget) => void;
}

/** Reihenfolge der sichtbaren Baumzeilen (Issue #425) — spiegelt die
 * Render-Reihenfolge; Gruppen klappen nicht zu, alle Zeilen sind sichtbar. */
function visibleKeys(
  tree: ReturnType<typeof buildGroupTree>,
  localServer: ServerDto | undefined,
): string[] {
  const keys: string[] = [];
  if (localServer) keys.push(`server:${localServer.id}`);
  const walk = (node: GroupTreeNode) => {
    keys.push(`group:${node.group.id}`);
    node.children.forEach(walk);
    node.servers.forEach((s) => keys.push(`server:${s.id}`));
    node.unusableServers.forEach((s) => keys.push(`unusableServer:${s.id}`));
  };
  tree.roots.forEach(walk);
  tree.ungroupedServers.forEach((s) => keys.push(`server:${s.id}`));
  tree.ungroupedUnusableServers.forEach((s) => keys.push(`unusableServer:${s.id}`));
  return keys;
}

/** Spec 0008, Abschnitt 6: rekursiv aus `list_groups()`/`list_servers()`
 * clientseitig aufgebauter Baum — Baum-Aufbau selbst über
 * `../groupTree`s `buildGroupTree` (Spec 0033, Abschnitt 5: dieselbe
 * Implementierung wie die gruppierte Hauptübersicht, kein zweiter
 * Nachbau). Der lokale Pseudo-Server (Spec 0032) ist Teil von `servers`,
 * aber NICHT Teil von `tree` (`buildGroupTree` klammert ihn bewusst aus
 * der Gruppen-/"Ohne Gruppe"-Struktur aus) — er wird hier separat, fix
 * oberhalb des Baums angeheftet dargestellt, sonst gäbe es in der
 * Verwalten-Ansicht keine Möglichkeit, seine Notizen/Tags zu bearbeiten
 * (Spec 0032, Abschnitt 3). */
export function Sidebar({
  groups,
  servers,
  unusableServers = [],
  selection,
  onSelect,
  onImportSshConfig,
  onExportSshConfig,
  onMove,
}: SidebarProps) {
  const { t, i18n } = useTranslation();
  const manageViews = useManageViews();
  const tree = buildGroupTree(groups, servers, unusableServers);
  const localServer = platformCapabilities(detectFallbackPlatform()).localServer
    ? servers.find((s) => s.isLocal)
    : undefined;
  const narrow = useIsNarrow();
  // Issue #585: row whose "Move to…" dialog is open (narrow layout).
  const [moveItem, setMoveItem] = useState<DragItem | null>(null);
  const { drag, handlersFor } = useTreeDrag(
    (item, target) => {
      if (classifyDrop(item, target, groups, servers) !== "noop") onMove(item, target);
    },
    { touchDrag: !narrow },
  );
  const moreButton = (item: DragItem) => (
    <Button
      variant="ghost"
      onClick={() => setMoveItem(item)}
      title={t("treeDrag.moveTo", { name: item.label })}
      aria-label={t("treeDrag.moveTo", { name: item.label })}
      className="mr-1 min-h-11 min-w-11 shrink-0"
    >
      ⋯
    </Button>
  );
  const dropKind = drag?.target ? classifyDrop(drag.item, drag.target, groups, servers) : null;
  const highlightFor = (target: DropTarget) =>
    drag?.target &&
    drag.target.kind === target.kind &&
    (target.kind === "root" || (drag.target.kind === "group" && drag.target.id === target.id))
      ? dropHighlightClass(dropKind)
      : "";

  // Issue #49: neue Elemente landen in der aktuellen Gruppe (ausgewählte
  // Gruppe bzw. Gruppe des ausgewählten Servers), s. `folderForNewItem`.
  const currentFolder = folderForNewItem(selection, servers, unusableServers);

  // Issue #425: Roving tabindex — genau eine Zeile ist Tab-Stopp. Ist die
  // zuletzt fokussierte Zeile verschwunden, gilt die Auswahl, sonst die erste.
  const treeRef = useRef<HTMLDivElement>(null);
  const [focusedKey, setFocusedKey] = useState<string | null>(null);
  const keys = [
    ...visibleKeys(tree, localServer),
    ...manageViews.map((v) => `extensionView:${v.id}`),
  ];
  const selectionKey = selection ? `${selection.kind}:${"id" in selection ? selection.id : ""}` : null;
  const tabStopKey =
    [focusedKey, selectionKey].find((k) => k !== null && keys.includes(k)) ?? keys[0] ?? null;

  const onTreeKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement;
    if (!target.hasAttribute("data-tree-key")) return;
    if (event.key === " " || event.key === "Enter") {
      event.preventDefault();
      target.click();
      return;
    }
    const rows = Array.from(
      treeRef.current?.querySelectorAll<HTMLElement>("[data-tree-key]") ?? [],
    );
    const index = rows.indexOf(target);
    let next: number;
    if (event.key === "ArrowDown") next = Math.min(index + 1, rows.length - 1);
    else if (event.key === "ArrowUp") next = Math.max(index - 1, 0);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = rows.length - 1;
    else return;
    event.preventDefault();
    rows[next]?.focus();
  };

  const tabIndexFor = (key: string) => (key === tabStopKey ? 0 : -1);

  /** Gemeinsame Attribute einer Baumzeile (Tree-Semantik + Roving tabindex). */
  const rowProps = (key: string, depth: number, selected: boolean) => ({
    // Narrow rows sit next to a "⋯" button, which a tree does not allow; the
    // narrow list is a plain list of buttons instead (issue #585).
    ...(narrow
      ? { "aria-current": selected ? ("true" as const) : undefined }
      : { "aria-level": depth + 1, "aria-selected": selected }),
    "data-tree-key": key,
    onFocus: () => setFocusedKey(key),
  });

  const isSelected = (kind: "group" | "server" | "unusableServer", id: string) =>
    selection?.kind === kind && selection.id === id;

  const renderNode = (node: GroupTreeNode, depth: number) => (
    <div
      key={node.group.id}
      role="none"
      data-drop-target={groupDropTargetValue(node.group.id)}
      className={`rounded ${highlightFor({ kind: "group", id: node.group.id })}`}
    >
      <div role={narrow ? "listitem" : "none"} className="flex items-center">
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the role is "button" in the narrow list and "treeitem" in the wide tree */}
      <div
      onKeyDown={onTreeKeyDown}
      role={narrow ? "button" : "treeitem"}
      tabIndex={tabIndexFor(`group:${node.group.id}`)}
      {...rowProps(`group:${node.group.id}`, depth, isSelected("group", node.group.id))}
        {...handlersFor({ kind: "group", id: node.group.id, label: node.group.name })}
        onClick={() => onSelect({ kind: "group", id: node.group.id })}
        title={node.group.name}
        style={{ paddingLeft: `${depth * 14 + 8}px` }}
        className={`block w-full min-w-0 cursor-pointer select-none truncate rounded px-2 py-1 text-left text-sm narrow:min-h-11 narrow:flex-1 narrow:py-3 hover:bg-surface-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-fg ${
          isSelected("group", node.group.id) ? "bg-surface-raised text-fg-bright" : "text-fg-muted"
        }`}
      >
        📁 {node.group.name}
      </div>
      {narrow && moreButton({ kind: "group", id: node.group.id, label: node.group.name })}
      </div>
      {node.children.map((child) => renderNode(child, depth + 1))}
      {node.servers.map((s) => renderServer(s, depth + 1))}
      {node.unusableServers.map((s) => renderUnusableServer(s, depth + 1))}
    </div>
  );

  const renderServer = (server: ServerDto, depth: number) => (
    <div key={server.id} role={narrow ? "listitem" : "none"} className="flex items-center">
    {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the role is "button" in the narrow list and "treeitem" in the wide tree */}
    <div
      onKeyDown={onTreeKeyDown}
      role={narrow ? "button" : "treeitem"}
      tabIndex={tabIndexFor(`server:${server.id}`)}
      {...rowProps(`server:${server.id}`, depth, isSelected("server", server.id))}
      // Issue #48: der lokale Pseudo-Server (Spec 0032) ist nicht ziehbar.
      {...(server.isLocal
        ? {}
        : handlersFor({ kind: "server", id: server.id, label: server.name }))}
      onClick={() => onSelect({ kind: "server", id: server.id })}
      title={server.name}
      style={{ paddingLeft: `${depth * 14 + 8}px` }}
      className={`block w-full min-w-0 cursor-pointer select-none truncate rounded px-2 py-1 text-left text-sm narrow:min-h-11 narrow:flex-1 narrow:py-3 hover:bg-surface-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-fg ${
        isSelected("server", server.id) ? "bg-surface-raised text-fg-bright" : "text-fg-muted"
      }`}
    >
      🖥️ {server.name}
    </div>
    {narrow && !server.isLocal && moreButton({ kind: "server", id: server.id, label: server.name })}
    </div>
  );

  /** Issue #100: auswählbar, damit das Löschen erreichbar ist — aber kein
   * Ziehen und kein Bearbeiten-Formular. */
  const renderUnusableServer = (server: UnusableServerDto, depth: number) => (
    <div key={server.id} role={narrow ? "listitem" : "none"}>
    {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the role is "button" in the narrow list and "treeitem" in the wide tree */}
    <div
      onKeyDown={onTreeKeyDown}
      role={narrow ? "button" : "treeitem"}
      tabIndex={tabIndexFor(`unusableServer:${server.id}`)}
      {...rowProps(`unusableServer:${server.id}`, depth, isSelected("unusableServer", server.id))}
      data-testid="unusable-server-entry"
      onClick={() => onSelect({ kind: "unusableServer", id: server.id })}
      title={unusableReasonText(t, server.reason)}
      style={{ paddingLeft: `${depth * 14 + 8}px` }}
      className={`block w-full min-w-0 cursor-pointer select-none truncate rounded px-2 py-1 text-left text-sm italic hover:bg-surface-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-fg ${
        isSelected("unusableServer", server.id) ? "bg-surface-raised text-warning-fg-softer" : "text-warning-fg-soft/80"
      }`}
    >
      <span aria-hidden="true">⚠️</span> {server.name}{" "}
      <span className="text-xs not-italic text-fg-subtle">({t("unusableServer.badge")})</span>
    </div>
    </div>
  );

  return (
    <div className="flex w-64 shrink-0 flex-col border-r border-surface-raised narrow:min-h-0 narrow:w-full narrow:flex-1 narrow:border-r-0">
      <div className="flex gap-1 border-b border-surface-raised p-2">
        <Button size="sm"
          onClick={() => onSelect({ kind: "newGroup", parentId: currentFolder })}
          className="flex-1"
        >
          {t("sidebar.addGroup")}
        </Button>
        <Button size="sm"
          onClick={() => onSelect({ kind: "newServer", groupId: currentFolder })}
          className="flex-1"
        >
          {t("sidebar.addServer")}
        </Button>
      </div>
      <div className="flex gap-1 border-b border-surface-raised p-2">
        <Button size="sm"
          onClick={onImportSshConfig}
          title={t("sidebar.importSshConfigHint")}
          className="flex-1"
        >
          {t("sidebar.importSshConfig")}
        </Button>
        <Button size="sm"
          onClick={onExportSshConfig}
          title={t("sidebar.exportSshConfigHint")}
          className="flex-1"
        >
          {t("sidebar.exportSshConfig")}
        </Button>
      </div>
      {/* Issue #48: der ganze Baumbereich ist das Ablageziel „Wurzelebene"
       * (ohne Gruppe / oberste Ebene); Gruppen darin sind innere Ziele.
       * Der angeheftete lokale Server schirmt seine Fläche ab (`none`). */}
      <div
        data-drop-target="root"
        className={`flex flex-1 flex-col overflow-y-auto p-2 ${highlightFor({ kind: "root" })}`}
      >
        {keys.length > 0 && (
          <div
            ref={treeRef}
            role={narrow ? "list" : "tree"}
            className="flex flex-col"
          >
        {localServer && (
          <div role="none" data-drop-target="none" className="mb-2 border-b border-surface-raised pb-2">
            {renderServer(localServer, 0)}
          </div>
        )}
        {tree.roots.map((node) => renderNode(node, 0))}
        {tree.ungroupedServers.map((s) => renderServer(s, 0))}
        {tree.ungroupedUnusableServers.map((s) => renderUnusableServer(s, 0))}
            {manageViews.length > 0 && (
              <div role="none" data-drop-target="none" className="mt-2 border-t border-surface-raised pt-2">
                {manageViews.map((view) => {
                  const key = `extensionView:${view.id}`;
                  const selected = selection?.kind === "extensionView" && selection.id === view.id;
                  const label = i18n.exists(view.label) ? t(view.label) : view.label;
                  return (
                    <div key={view.id} role={narrow ? "listitem" : "none"}>
                    {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the role is "button" in the narrow list and "treeitem" in the wide tree */}
                    <div
                      onKeyDown={onTreeKeyDown}
                      role={narrow ? "button" : "treeitem"}
                      tabIndex={tabIndexFor(key)}
                      {...rowProps(key, 0, selected)}
                      data-testid="extension-view-entry"
                      onClick={() => onSelect({ kind: "extensionView", id: view.id })}
                      title={label}
                      style={{ paddingLeft: "8px" }}
                      className={`block w-full min-w-0 cursor-pointer select-none truncate rounded px-2 py-1 text-left text-sm narrow:min-h-11 narrow:flex-1 narrow:py-3 hover:bg-surface-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-fg ${
                        selected ? "bg-surface-raised text-fg-bright" : "text-fg-muted"
                      }`}
                    >
                      {view.icon && <span aria-hidden="true">{view.icon} </span>}
                      {label}
                    </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}
        {groups.length === 0 &&
          tree.ungroupedServers.length === 0 &&
          tree.ungroupedUnusableServers.length === 0 && (
          <p className="px-2 py-1 text-sm text-fg-subtle">{t("sidebar.empty")}</p>
        )}
        {drag && (
          <p className="mt-2 rounded border border-dashed border-border-strong px-2 py-2 text-xs text-fg-subtle">
            {t("treeDrag.rootDropZone")}
          </p>
        )}
        {/* Füllt den Rest, damit auch die freie Fläche unter dem Baum als
         * Wurzel-Ziel greift. */}
        <div className="min-h-4 flex-1" />
      </div>
      {moveItem && (
        <MoveToDialog
          item={moveItem}
          groups={groups}
          servers={servers}
          onMove={onMove}
          onClose={() => setMoveItem(null)}
        />
      )}
      {drag && <TreeDragGhost drag={drag} kind={dropKind} />}
    </div>
  );
}
