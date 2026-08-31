import { useEffect, useState } from "react";
import { commandErrorMessage, listGroups, listServers } from "../api";
import type { GroupDto, ServerDto } from "../types";
import { GroupForm } from "./GroupForm";
import { ServerForm } from "./ServerForm";
import { Sidebar, type Selection } from "./Sidebar";

/** Spec 0008, Abschnitt 6: Sidebar links, Formular im Hauptbereich. */
export function ManagementView() {
  const [groups, setGroups] = useState<GroupDto[]>([]);
  const [servers, setServers] = useState<ServerDto[]>([]);
  const [selection, setSelection] = useState<Selection | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = () => {
    Promise.all([listGroups(), listServers()])
      .then(([g, s]) => {
        setGroups(g);
        setServers(s);
      })
      .catch((err) => setError(commandErrorMessage(err)));
  };

  useEffect(reload, []);

  const handleDeleted = () => {
    setSelection(null);
    reload();
  };

  const handleCreated = () => {
    setSelection(null);
    reload();
  };

  return (
    <div className="flex min-h-0 flex-1">
      <Sidebar groups={groups} servers={servers} selection={selection} onSelect={setSelection} />
      <div className="flex-1 overflow-y-auto">
        {error && <p className="p-4 text-sm text-red-400">{error}</p>}

        {selection?.kind === "group" && (
          <GroupForm
            groupId={selection.id}
            defaultParentId={null}
            allGroups={groups}
            onSaved={reload}
            onDeleted={handleDeleted}
          />
        )}
        {selection?.kind === "newGroup" && (
          <GroupForm
            groupId={null}
            defaultParentId={selection.parentId}
            allGroups={groups}
            onSaved={handleCreated}
            onDeleted={handleDeleted}
          />
        )}
        {selection?.kind === "server" && (
          <ServerForm
            serverId={selection.id}
            defaultGroupId={null}
            allGroups={groups}
            allServers={servers}
            onSaved={reload}
            onDeleted={handleDeleted}
          />
        )}
        {selection?.kind === "newServer" && (
          <ServerForm
            serverId={null}
            defaultGroupId={selection.groupId}
            allGroups={groups}
            allServers={servers}
            onSaved={handleCreated}
            onDeleted={handleDeleted}
          />
        )}
        {!selection && (
          <p className="p-4 text-sm text-slate-400">
            Links eine Gruppe oder einen Server auswählen, oder über "+ Gruppe"/"+ Server" etwas Neues
            anlegen.
          </p>
        )}
      </div>
    </div>
  );
}
