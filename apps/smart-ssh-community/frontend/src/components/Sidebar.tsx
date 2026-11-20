import { dynamicKey } from "../i18nTypes";
import { useEffect, useRef, useState } from "react";
import { detectFallbackPlatform, platformCapabilities } from "../lib/platform";
import { useTranslation } from "react-i18next";
import { buildGroupTree, groupItemCount, type GroupTreeNode } from "../lib/groupTree";
import { folderForNewItem } from "../lib/newItemFolder";
import {
  classifyDrop,
  dropHighlightClass,
  groupDropTargetValue,
  type DragItem,
  type DropTarget,
} from "../lib/treeDrag";
import type { GroupDto, KeepGroupResult, ServerDto, UnusableServerDto } from "../api/types";
import { keepGroupAsRegularEntry, keepServerAsRegularEntry } from "../api/api";
import { commandErrorText } from "../api/errorCodes";
import { useTreeDrag } from "../hooks/useTreeDrag";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { MoveToDialog } from "./MoveToDialog";
import { useManageViews } from "../extensions/FormSlots";
import { TreeDragGhost } from "./TreeDragGhost";
import { unusableReasonText } from "../lib/unusableServer";
import { Badge, Button, DialogFrame, Icon } from "./ui";

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
  /** Spec 0103 §3a: a copy entry was kept as a regular entry — reload the tree. */
  onKeptAsRegular?: () => void;
  /** Issue #1368: collapsed groups, kept by the caller so the state survives
   * leaving the Manage view (shared with the Connect tree). Without it the
   * sidebar keeps its own state. */
  collapsedGroupIds?: ReadonlySet<string>;
  onToggleGroup?: (groupId: string) => void;
}

/** Container of a group's children: a 1 px rule under the parent's chevron
 * centre (20 px wide, 22 px narrow), as in the Connect tree. */
const CHILDREN_CLASS = "ml-5 border-l border-border narrow:ml-[22px]";

/** Short horizontal connector from the rule to a nested row (decorative). */
function TreeTick() {
  return (
    <span
      aria-hidden="true"
      data-testid="tree-tick"
      className="pointer-events-none absolute left-0 top-[15px] h-px w-3 bg-border narrow:top-[22px]"
    />
  );
}

/** Flat selected row: `accent-subtle` with an inset 1 px `accent-border`. */
const SELECTED_ROW = "bg-accent-subtle shadow-[inset_0_0_0_1px_var(--accent-border)] text-fg-bright";

/** Reihenfolge der sichtbaren Baumzeilen (Issue #425) — spiegelt die
 * Render-Reihenfolge; zugeklappte Gruppen blenden ihre Kinder aus (Issue #1368). */
function visibleKeys(
  tree: ReturnType<typeof buildGroupTree>,
  localServer: ServerDto | undefined,
  collapsed: ReadonlySet<string>,
): string[] {
  const keys: string[] = [];
  if (localServer) keys.push(`server:${localServer.id}`);
  const walk = (node: GroupTreeNode) => {
    keys.push(`group:${node.group.id}`);
    if (collapsed.has(node.group.id)) return;
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
 * `../lib/groupTree`s `buildGroupTree` (Spec 0033, Abschnitt 5: dieselbe
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
  onKeptAsRegular,
  collapsedGroupIds,
  onToggleGroup,
}: SidebarProps) {
  const { t, i18n } = useTranslation();
  const manageViews = useManageViews();
  const tree = buildGroupTree(groups, servers, unusableServers);
  const localServer = platformCapabilities(detectFallbackPlatform()).localServer
    ? servers.find((s) => s.isLocal)
    : undefined;
  const narrow = useIsNarrow();
  // Issue #1368: collapse state — the caller's if given, else local.
  const [localCollapsed, setLocalCollapsed] = useState<ReadonlySet<string>>(new Set());
  const collapsed = collapsedGroupIds ?? localCollapsed;
  const toggleGroup = (id: string) => {
    if (onToggleGroup) {
      onToggleGroup(id);
      return;
    }
    setLocalCollapsed((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  };
  const setGroupCollapsed = (id: string, wantCollapsed: boolean) => {
    if (collapsed.has(id) !== wantCollapsed) toggleGroup(id);
  };
  // A selected item inside a collapsed group expands its ancestors (once per
  // selection, so collapsing the group again afterwards sticks).
  const expandedFor = useRef<string | null>(null);
  // Issue #585: row whose "Move to…" dialog is open (narrow layout).
  const [moveItem, setMoveItem] = useState<DragItem | null>(null);
  // Spec 0103 §3a: "Keep as regular entry" for copy rows, from the "⋯" dialog.
  const [keepError, setKeepError] = useState<string | null>(null);
  const [keepGroup, setKeepGroup] = useState<{ group: GroupDto; scope: KeepGroupResult } | null>(
    null,
  );
  const keepServer = async (id: string) => {
    setKeepError(null);
    try {
      await keepServerAsRegularEntry(id);
    } catch (err) {
      setKeepError(commandErrorText(t, err));
    }
    onKeptAsRegular?.();
  };
  const askKeepGroup = async (group: GroupDto) => {
    setKeepError(null);
    try {
      setKeepGroup({ group, scope: await keepGroupAsRegularEntry(group.id, false) });
    } catch (err) {
      setKeepError(commandErrorText(t, err));
    }
  };
  const confirmKeepGroup = async () => {
    if (!keepGroup) return;
    const { group } = keepGroup;
    setKeepGroup(null);
    try {
      await keepGroupAsRegularEntry(group.id, true);
    } catch (err) {
      setKeepError(commandErrorText(t, err));
    }
    onKeptAsRegular?.();
  };
  const keepActionFor = (item: DragItem): (() => void) | undefined => {
    if (item.kind === "server") {
      const server = servers.find((s) => s.id === item.id);
      return server?.isCopy ? () => void keepServer(item.id) : undefined;
    }
    const group = groups.find((g) => g.id === item.id);
    return group?.isCopy ? () => void askKeepGroup(group) : undefined;
  };
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
      <Icon name="more" />
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
    ...visibleKeys(tree, localServer, collapsed),
    ...manageViews.map((v) => `extensionView:${v.id}`),
  ];
  const selectionKey = selection ? `${selection.kind}:${"id" in selection ? selection.id : ""}` : null;
  const tabStopKey =
    [focusedKey, selectionKey].find((k) => k !== null && keys.includes(k)) ?? keys[0] ?? null;

  // Issue #1368: expand the ancestors of a newly selected item.
  const selectionGroupChain = (): string[] | null => {
    if (!selection) return null;
    let start: string | null;
    if (selection.kind === "group") {
      start = groups.find((g) => g.id === selection.id)?.parentId ?? null;
      if (!groups.some((g) => g.id === selection.id)) return null;
    } else if (selection.kind === "server") {
      const s = servers.find((x) => x.id === selection.id);
      if (!s) return null;
      start = s.groupId;
    } else if (selection.kind === "unusableServer") {
      const s = unusableServers.find((x) => x.id === selection.id);
      if (!s) return null;
      start = s.groupId;
    } else if (selection.kind === "newGroup") start = selection.parentId;
    else if (selection.kind === "newServer") start = selection.groupId;
    else return null;
    const chain: string[] = [];
    while (start && !chain.includes(start)) {
      chain.push(start);
      start = groups.find((g) => g.id === start)?.parentId ?? null;
    }
    return chain;
  };
  useEffect(() => {
    if (expandedFor.current === selectionKey) return;
    const chain = selectionGroupChain();
    if (chain === null) {
      // Not resolvable yet (data still loading) or nothing to expand.
      if (!selection || !("id" in selection)) expandedFor.current = selectionKey;
      return;
    }
    expandedFor.current = selectionKey;
    chain.forEach((id) => setGroupCollapsed(id, false));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs when the selection or the loaded data changes; helpers are recreated each render
  }, [selectionKey, groups, servers, unusableServers]);

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
    const groupId = target.getAttribute("data-group-id");
    if (event.key === "ArrowRight" && groupId) {
      event.preventDefault();
      if (collapsed.has(groupId)) setGroupCollapsed(groupId, false);
      else if (rows[index + 1]?.getAttribute("data-parent-key") === target.getAttribute("data-tree-key"))
        rows[index + 1].focus();
      return;
    }
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      if (groupId && !collapsed.has(groupId)) {
        setGroupCollapsed(groupId, true);
        return;
      }
      const parentKey = target.getAttribute("data-parent-key");
      rows.find((r) => r.getAttribute("data-tree-key") === parentKey)?.focus();
      return;
    }
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
  const rowProps = (key: string, depth: number, selected: boolean, parentGroupId?: string | null) => ({
    // Narrow rows sit next to a "⋯" button, which a tree does not allow; the
    // narrow list is a plain list of buttons instead (issue #585).
    ...(narrow
      ? { "aria-current": selected ? ("true" as const) : undefined }
      : { "aria-level": depth + 1, "aria-selected": selected }),
    "data-tree-key": key,
    "data-parent-key": parentGroupId ? `group:${parentGroupId}` : undefined,
    onFocus: () => setFocusedKey(key),
  });

  const isSelected = (kind: "group" | "server" | "unusableServer", id: string) =>
    selection?.kind === kind && selection.id === id;

  const ROW_BASE =
    "flex w-full min-w-0 cursor-pointer select-none items-center gap-2 py-1 pr-3 text-left text-sm min-h-[30px] narrow:min-h-11 narrow:flex-1 narrow:py-3 hover:bg-surface-raised focus-ring-inset";
  const padFor = (depth: number) => (depth > 0 ? "pl-5" : "pl-3");

  const renderNode = (node: GroupTreeNode, depth: number, parentId: string | null) => {
    const isCollapsed = collapsed.has(node.group.id);
    const selected = isSelected("group", node.group.id);
    const name = node.group.name;
    const chevron = (
      <Icon name={isCollapsed ? "chev-r" : "chev-d"} size={narrow ? 20 : 16} />
    );
    return (
      <div
        key={node.group.id}
        role="none"
        data-drop-target={groupDropTargetValue(node.group.id)}
        className={`relative rounded ${highlightFor({ kind: "group", id: node.group.id })}`}
      >
        {depth > 0 && <TreeTick />}
        <div role={narrow ? "listitem" : "none"} className="flex items-center border-b border-border">
          {narrow && (
            <Button
              variant="bare"
              onClick={() => toggleGroup(node.group.id)}
              aria-expanded={!isCollapsed}
              aria-label={t(isCollapsed ? "sidebar.expandGroup" : "sidebar.collapseGroup", { name })}
              className="flex min-h-11 min-w-11 shrink-0 items-center justify-center text-fg-soft focus-ring-inset"
            >
              {chevron}
            </Button>
          )}
          {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the role is "button" in the narrow list and "treeitem" in the wide tree */}
          <div
            onKeyDown={onTreeKeyDown}
            role={narrow ? "button" : "treeitem"}
            tabIndex={tabIndexFor(`group:${node.group.id}`)}
            {...rowProps(`group:${node.group.id}`, depth, selected, parentId)}
            {...(narrow ? {} : { "aria-expanded": !isCollapsed })}
            data-group-id={node.group.id}
            {...handlersFor({ kind: "group", id: node.group.id, label: name })}
            onClick={() => onSelect({ kind: "group", id: node.group.id })}
            title={name}
            className={`${ROW_BASE} font-semibold ${narrow ? "pl-1" : padFor(depth)} ${
              selected ? SELECTED_ROW : "text-fg-soft"
            }`}
          >
            {!narrow && (
              // Mouse-only toggle; the keyboard uses Left/Right on the row.
              // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions -- keyboard: Arrow Left/Right on the treeitem
              <span
                data-testid="group-chevron"
                aria-hidden="true"
                className="inline-flex shrink-0 text-fg-soft"
                onClick={(e) => {
                  e.stopPropagation();
                  toggleGroup(node.group.id);
                }}
              >
                {chevron}
              </span>
            )}
            <Icon name={isCollapsed ? "folder" : "folder-open"} className="shrink-0 text-fg-muted" />
            <span className="truncate">{name}</span>
            <span
              data-testid="group-count"
              className="ml-auto shrink-0 font-mono text-xs font-normal text-fg-faint"
            >
              {groupItemCount(node)}
            </span>
          </div>
          {narrow && moreButton({ kind: "group", id: node.group.id, label: name })}
        </div>
        {!isCollapsed && groupItemCount(node) > 0 && (
          <div role="none" data-testid="tree-children" className={CHILDREN_CLASS}>
            {node.children.map((child) => renderNode(child, depth + 1, node.group.id))}
            {node.servers.map((s) => renderServer(s, depth + 1, node.group.id))}
            {node.unusableServers.map((s) => renderUnusableServer(s, depth + 1, node.group.id))}
          </div>
        )}
      </div>
    );
  };

  const renderServer = (server: ServerDto, depth: number, parentId: string | null = null) => {
    const selected = isSelected("server", server.id);
    return (
      <div
        key={server.id}
        role={narrow ? "listitem" : "none"}
        className="relative flex items-center border-b border-border"
      >
        {depth > 0 && <TreeTick />}
        {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the role is "button" in the narrow list and "treeitem" in the wide tree */}
        <div
          onKeyDown={onTreeKeyDown}
          role={narrow ? "button" : "treeitem"}
          tabIndex={tabIndexFor(`server:${server.id}`)}
          {...rowProps(`server:${server.id}`, depth, selected, parentId)}
          // Issue #48: der lokale Pseudo-Server (Spec 0032) ist nicht ziehbar.
          {...(server.isLocal
            ? {}
            : handlersFor({ kind: "server", id: server.id, label: server.name }))}
          onClick={() => onSelect({ kind: "server", id: server.id })}
          title={server.name}
          className={`${ROW_BASE} ${padFor(depth)} ${selected ? SELECTED_ROW : "text-fg-muted"}`}
        >
          <Icon name={server.isLocal ? "local" : "server"} className="shrink-0 text-fg-muted" />
          <span className="truncate font-mono">{server.name}</span>
          {server.isLocal && (
            <Badge tone="local" className="ml-auto shrink-0">
              {t("mainScreen.localBadge")}
            </Badge>
          )}
        </div>
        {narrow && !server.isLocal && moreButton({ kind: "server", id: server.id, label: server.name })}
      </div>
    );
  };

  /** Issue #100: auswählbar, damit das Löschen erreichbar ist — aber kein
   * Ziehen und kein Bearbeiten-Formular. */
  const renderUnusableServer = (
    server: UnusableServerDto,
    depth: number,
    parentId: string | null = null,
  ) => {
    const selected = isSelected("unusableServer", server.id);
    return (
      <div key={server.id} role={narrow ? "listitem" : "none"} className="relative border-b border-border">
        {depth > 0 && <TreeTick />}
        {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the role is "button" in the narrow list and "treeitem" in the wide tree */}
        <div
          onKeyDown={onTreeKeyDown}
          role={narrow ? "button" : "treeitem"}
          tabIndex={tabIndexFor(`unusableServer:${server.id}`)}
          {...rowProps(`unusableServer:${server.id}`, depth, selected, parentId)}
          data-testid="unusable-server-entry"
          onClick={() => onSelect({ kind: "unusableServer", id: server.id })}
          title={unusableReasonText(t, server.reason)}
          className={`${ROW_BASE} ${padFor(depth)} italic ${
            selected ? `${SELECTED_ROW} text-warning-fg-softer` : "text-warning-fg-soft/80"
          }`}
        >
          <Icon name="warn" className="shrink-0" />
          <span className="truncate">{server.name}</span>
          <span className="ml-auto shrink-0 text-xs not-italic text-fg-subtle">
            ({t("unusableServer.badge")})
          </span>
        </div>
      </div>
    );
  };

  return (
    <div className="flex w-72 shrink-0 flex-col border-r border-border bg-surface-sunken narrow:min-h-0 narrow:w-full narrow:flex-1 narrow:border-r-0">
      <div className="flex min-h-12 items-center justify-between gap-2 border-b border-border px-3">
        <span className="text-label">{t("sidebar.title")}</span>
        <div className="flex gap-1.5">
          <Button
            size="sm"
            variant="secondary"
            onClick={() => onSelect({ kind: "newGroup", parentId: currentFolder })}
          >
            {t("sidebar.addGroup")}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => onSelect({ kind: "newServer", groupId: currentFolder })}
          >
            {t("sidebar.addServer")}
          </Button>
        </div>
      </div>
      {/* Issue #48: der ganze Baumbereich ist das Ablageziel „Wurzelebene"
       * (ohne Gruppe / oberste Ebene); Gruppen darin sind innere Ziele.
       * Der angeheftete lokale Server schirmt seine Fläche ab (`none`). */}
      <div
        data-drop-target="root"
        className={`flex flex-1 flex-col overflow-y-auto ${highlightFor({ kind: "root" })}`}
      >
        {keys.length > 0 && (
          <div
            ref={treeRef}
            role={narrow ? "list" : "tree"}
            className="flex flex-col"
          >
        {localServer && (
          <div role="none" data-drop-target="none">
            {renderServer(localServer, 0)}
          </div>
        )}
        {tree.roots.map((node) => renderNode(node, 0, null))}
        {tree.ungroupedServers.map((s) => renderServer(s, 0))}
        {tree.ungroupedUnusableServers.map((s) => renderUnusableServer(s, 0))}
            {manageViews.length > 0 && (
              <div role="none" data-drop-target="none" className="mt-2 border-t border-border pt-2">
                {manageViews.map((view) => {
                  const key = `extensionView:${view.id}`;
                  const selected = selection?.kind === "extensionView" && selection.id === view.id;
                  const label = i18n.exists(view.label) ? t(dynamicKey(view.label)) : view.label;
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
                      className={`${ROW_BASE} pl-3 ${selected ? SELECTED_ROW : "text-fg-muted"}`}
                    >
                      {view.icon && <span aria-hidden="true">{view.icon}</span>}
                      <span className="truncate">{label}</span>
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
          <p className="px-3 py-2 text-sm text-fg-subtle">{t("sidebar.empty")}</p>
        )}
        {drag && (
          <p className="m-2 border border-dashed border-border-strong px-2 py-2 text-xs text-fg-subtle">
            {t("treeDrag.rootDropZone")}
          </p>
        )}
        {/* Füllt den Rest, damit auch die freie Fläche unter dem Baum als
         * Wurzel-Ziel greift. */}
        <div className="min-h-4 flex-1" />
      </div>
      <div className="grid grid-cols-2 border-t border-border">
        <Button
          variant="ghost"
          onClick={onImportSshConfig}
          title={t("sidebar.importSshConfigHint")}
          className="h-11 rounded-none"
        >
          <Icon name="import" />
          {t("sidebar.importSshConfig")}
        </Button>
        <Button
          variant="ghost"
          onClick={onExportSshConfig}
          title={t("sidebar.exportSshConfigHint")}
          className="h-11 rounded-none border-l border-border"
        >
          <Icon name="export" />
          {t("sidebar.exportSshConfig")}
        </Button>
      </div>
      {moveItem && (
        <MoveToDialog
          item={moveItem}
          groups={groups}
          servers={servers}
          onMove={onMove}
          onKeepAsRegular={keepActionFor(moveItem)}
          onClose={() => setMoveItem(null)}
        />
      )}
      {keepError && (
        <p role="alert" className="px-2 py-1 text-xs text-danger-fg">
          {keepError}
        </p>
      )}
      {keepGroup && (
        <DialogFrame
          layer="app"
          title={<span className="break-words">{t("copyMarker.keep")}</span>}
          onClose={() => setKeepGroup(null)}
          actions={
            <>
              <Button onClick={() => setKeepGroup(null)}>{t("common.cancel")}</Button>
              <Button variant="primary" onClick={() => void confirmKeepGroup()}>
                {t("copyMarker.keep")}
              </Button>
            </>
          }
        >
          <p className="break-words text-sm text-fg-soft"><Icon name="folder" className="inline align-text-bottom" /> {keepGroup.group.name}</p>
          <p className="mt-2 text-sm text-fg-muted">
            {t("copyMarker.groupConfirmBody", {
              groups: keepGroup.scope.groups,
              servers: keepGroup.scope.servers,
            })}
          </p>
        </DialogFrame>
      )}
      {drag && <TreeDragGhost drag={drag} kind={dropKind} />}
    </div>
  );
}
