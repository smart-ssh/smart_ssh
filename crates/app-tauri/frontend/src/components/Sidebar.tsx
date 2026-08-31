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
 * clientseitig aufgebauter Baum. */
export function Sidebar({ groups, servers, selection, onSelect }: SidebarProps) {
  const childGroupsOf = (parentId: string | null) =>
    groups.filter((g) => g.parentId === parentId);
  const serversOf = (groupId: string | null) => servers.filter((s) => s.groupId === groupId);

  const isSelected = (kind: "group" | "server", id: string) =>
    selection?.kind === kind && selection.id === id;

  const renderGroup = (group: GroupDto, depth: number) => (
    <div key={group.id}>
      <div
        role="button"
        tabIndex={0}
        onClick={() => onSelect({ kind: "group", id: group.id })}
        style={{ paddingLeft: `${depth * 14 + 8}px` }}
        className={`cursor-pointer truncate rounded px-2 py-1 text-sm hover:bg-slate-800 ${
          isSelected("group", group.id) ? "bg-slate-800 text-white" : "text-slate-300"
        }`}
      >
        📁 {group.name}
      </div>
      {childGroupsOf(group.id).map((g) => renderGroup(g, depth + 1))}
      {serversOf(group.id).map((s) => renderServer(s, depth + 1))}
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
          + Gruppe
        </button>
        <button
          type="button"
          onClick={() => onSelect({ kind: "newServer", groupId: null })}
          className="flex-1 rounded bg-slate-800 px-2 py-1 text-xs hover:bg-slate-700"
        >
          + Server
        </button>
      </div>
      <div className="flex-1 overflow-y-auto p-2">
        {childGroupsOf(null).map((g) => renderGroup(g, 0))}
        {serversOf(null).map((s) => renderServer(s, 0))}
        {groups.length === 0 && servers.length === 0 && (
          <p className="px-2 py-1 text-sm text-slate-500">Noch nichts angelegt.</p>
        )}
      </div>
    </div>
  );
}
