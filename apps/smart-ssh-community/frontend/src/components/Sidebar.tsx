import { useTranslation } from "react-i18next";
import { buildGroupTree, type GroupTreeNode } from "../groupTree";
import type { GroupDto, ServerDto } from "../types";

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
export function Sidebar({ groups, servers, selection, onSelect }: SidebarProps) {
  const { t } = useTranslation();
  const tree = buildGroupTree(groups, servers);
  const localServer = servers.find((s) => s.isLocal);

  const isSelected = (kind: "group" | "server", id: string) =>
    selection?.kind === kind && selection.id === id;

  const renderNode = (node: GroupTreeNode, depth: number) => (
    <div key={node.group.id}>
      <div
        role="button"
        tabIndex={0}
        onClick={() => onSelect({ kind: "group", id: node.group.id })}
        style={{ paddingLeft: `${depth * 14 + 8}px` }}
        className={`cursor-pointer truncate rounded px-2 py-1 text-sm hover:bg-slate-800 ${
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
      onClick={() => onSelect({ kind: "server", id: server.id })}
      style={{ paddingLeft: `${depth * 14 + 8}px` }}
      className={`cursor-pointer truncate rounded px-2 py-1 text-sm hover:bg-slate-800 ${
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
      <div className="flex-1 overflow-y-auto p-2">
        {localServer && (
          <div className="mb-2 border-b border-slate-800 pb-2">{renderServer(localServer, 0)}</div>
        )}
        {tree.roots.map((node) => renderNode(node, 0))}
        {tree.ungroupedServers.map((s) => renderServer(s, 0))}
        {groups.length === 0 && tree.ungroupedServers.length === 0 && (
          <p className="px-2 py-1 text-sm text-slate-500">{t("sidebar.empty")}</p>
        )}
      </div>
    </div>
  );
}
