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

        {/* `key` erzwingt einen vollständigen Remount (statt Wiederverwendung
         * derselben Komponenten-Instanz mit nur geänderten Props), sobald
         * eine andere Gruppe/ein anderer Server ausgewählt wird — sonst
         * bleibt z. B. `ServerForm`s eigener `loaded`-State (aus `getServer`)
         * bis zum Abschluss des nächsten Fetches auf dem vorherigen Server
         * stehen, und `NotesPanel`s `showHistory`/`revisions`-State bleibt
         * über den Wechsel hinweg fälschlich erhalten. Klassischer
         * React-Fallstrick bei direktem A→B-Wechsel ohne Zwischenzustand
         * (kein zwischenzeitliches Unmounten), s. Commit
         * "fix(app-tauri): load notes and revision history correctly in
         * server form". */}
        {selection?.kind === "group" && (
          <GroupForm
            key={selection.id}
            groupId={selection.id}
            defaultParentId={null}
            allGroups={groups}
            onSaved={reload}
            onDeleted={handleDeleted}
          />
        )}
        {selection?.kind === "newGroup" && (
          <GroupForm
            key={`new-${selection.parentId ?? "root"}`}
            groupId={null}
            defaultParentId={selection.parentId}
            allGroups={groups}
            onSaved={handleCreated}
            onDeleted={handleDeleted}
          />
        )}
        {selection?.kind === "server" && (
          <ServerForm
            key={selection.id}
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
            key={`new-${selection.groupId ?? "root"}`}
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
