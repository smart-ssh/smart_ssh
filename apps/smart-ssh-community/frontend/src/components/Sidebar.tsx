import { useTranslation } from "react-i18next";
import { buildGroupTree, type GroupTreeNode } from "../groupTree";
import {
  classifyDrop,
  dropHighlightClass,
  groupDropTargetValue,
  type DragItem,
  type DropTarget,
} from "../treeDrag";
import type { GroupDto, ServerDto } from "../types";
import { useTreeDrag } from "../useTreeDrag";
import { TreeDragGhost } from "./TreeDragGhost";

export type Selection =
  | { kind: "group"; id: string }
  | { kind: "server"; id: string }
  | { kind: "newGroup"; parentId: string | null }
  | { kind: "newServer"; groupId: string | null };

interface SidebarProps {
  groups: GroupDto[];
  servers: ServerDto[];
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
  selection,
  onSelect,
  onImportSshConfig,
  onExportSshConfig,
  onMove,
}: SidebarProps) {
  const { t } = useTranslation();
  const tree = buildGroupTree(groups, servers);
  const localServer = servers.find((s) => s.isLocal);
  const { drag, handlersFor } = useTreeDrag((item, target) => {
    if (classifyDrop(item, target, groups, servers) !== "noop") onMove(item, target);
  });
  const dropKind = drag?.target ? classifyDrop(drag.item, drag.target, groups, servers) : null;
  const highlightFor = (target: DropTarget) =>
    drag?.target &&
    drag.target.kind === target.kind &&
    (target.kind === "root" || (drag.target.kind === "group" && drag.target.id === target.id))
      ? dropHighlightClass(dropKind)
      : "";

  const isSelected = (kind: "group" | "server", id: string) =>
    selection?.kind === kind && selection.id === id;

  const renderNode = (node: GroupTreeNode, depth: number) => (
    <div
      key={node.group.id}
      data-drop-target={groupDropTargetValue(node.group.id)}
      className={`rounded ${highlightFor({ kind: "group", id: node.group.id })}`}
    >
      <div
        role="button"
        tabIndex={0}
        {...handlersFor({ kind: "group", id: node.group.id, label: node.group.name })}
        onClick={() => onSelect({ kind: "group", id: node.group.id })}
        style={{ paddingLeft: `${depth * 14 + 8}px` }}
        className={`cursor-pointer select-none truncate rounded px-2 py-1 text-sm hover:bg-slate-800 ${
          isSelected("group", node.group.id) ? "bg-slate-800 text-white" : "text-slate-300"
        }`}
      >
        📁 {node.group.name}
      </div>
      {node.children.map((child) => renderNode(child, depth + 1))}
      {node.servers.map((s) => renderServer(s, depth + 1))}
    </div>
  );

  const renderServer = (server: ServerDto, depth: number) => (
    <div
      key={server.id}
      role="button"
      tabIndex={0}
      // Issue #48: der lokale Pseudo-Server (Spec 0032) ist nicht ziehbar.
      {...(server.isLocal
        ? {}
        : handlersFor({ kind: "server", id: server.id, label: server.name }))}
      onClick={() => onSelect({ kind: "server", id: server.id })}
      style={{ paddingLeft: `${depth * 14 + 8}px` }}
      className={`cursor-pointer select-none truncate rounded px-2 py-1 text-sm hover:bg-slate-800 ${
        isSelected("server", server.id) ? "bg-slate-800 text-white" : "text-slate-300"
      }`}
    >
      🖥️ {server.name}
    </div>
  );

  return (
    <div className="flex w-64 shrink-0 flex-col border-r border-slate-800">
      <div className="flex gap-1 border-b border-slate-800 p-2">
        <button
          type="button"
          onClick={() => onSelect({ kind: "newGroup", parentId: null })}
          className="flex-1 rounded bg-slate-800 px-2 py-1 text-xs hover:bg-slate-700"
        >
          {t("sidebar.addGroup")}
        </button>
        <button
          type="button"
          onClick={() => onSelect({ kind: "newServer", groupId: null })}
          className="flex-1 rounded bg-slate-800 px-2 py-1 text-xs hover:bg-slate-700"
        >
          {t("sidebar.addServer")}
        </button>
      </div>
      <div className="flex gap-1 border-b border-slate-800 p-2">
        <button
          type="button"
          onClick={onImportSshConfig}
          title={t("sidebar.importSshConfigHint")}
          className="flex-1 rounded border border-slate-700 px-2 py-1 text-xs text-slate-300 hover:bg-slate-800"
        >
          {t("sidebar.importSshConfig")}
        </button>
        <button
          type="button"
          onClick={onExportSshConfig}
          title={t("sidebar.exportSshConfigHint")}
          className="flex-1 rounded border border-slate-700 px-2 py-1 text-xs text-slate-300 hover:bg-slate-800"
        >
          {t("sidebar.exportSshConfig")}
        </button>
      </div>
      {/* Issue #48: der ganze Baumbereich ist das Ablageziel „Wurzelebene"
       * (ohne Gruppe / oberste Ebene); Gruppen darin sind innere Ziele.
       * Der angeheftete lokale Server schirmt seine Fläche ab (`none`). */}
      <div
        data-drop-target="root"
        className={`flex flex-1 flex-col overflow-y-auto p-2 ${highlightFor({ kind: "root" })}`}
      >
        {localServer && (
          <div data-drop-target="none" className="mb-2 border-b border-slate-800 pb-2">
            {renderServer(localServer, 0)}
          </div>
        )}
        {tree.roots.map((node) => renderNode(node, 0))}
        {tree.ungroupedServers.map((s) => renderServer(s, 0))}
        {groups.length === 0 && tree.ungroupedServers.length === 0 && (
          <p className="px-2 py-1 text-sm text-slate-500">{t("sidebar.empty")}</p>
        )}
        {drag && (
          <p className="mt-2 rounded border border-dashed border-slate-600 px-2 py-2 text-xs text-slate-400">
            {t("treeDrag.rootDropZone")}
          </p>
        )}
        {/* Füllt den Rest, damit auch die freie Fläche unter dem Baum als
         * Wurzel-Ziel greift. */}
        <div className="min-h-4 flex-1" />
      </div>
      {drag && <TreeDragGhost drag={drag} kind={dropKind} />}
    </div>
  );
}
