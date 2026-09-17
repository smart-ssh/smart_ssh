import { useEffect, useState } from "react";
import { commandErrorMessage, listGroups, listServers } from "../api";
import type { GroupDto, ServerDto } from "../types";
import { GroupForm } from "./GroupForm";
import { ServerForm } from "./ServerForm";
import { Sidebar, type Selection } from "./Sidebar";

interface ManagementViewProps {
  /** Spec 0057, §4.2 (Etappe 4): "Mache ich selbst" im Kürzungs-Vorschlags-
   * Dialog (`NoteShrinkSuggestionToast`, gemountet außerhalb dieser
   * Komponente) springt direkt zur Server-Bearbeitung eines bestimmten
   * Servers — übergeben von `App.tsx` über den `navigationBus`. `null`/
   * `undefined` im Normalfall (regulärer Aufruf über die Navigation). */
  initialSelection?: Selection | null;
  /** spec-reviewer-Fund (Review dieses Schritts): ohne diesen Rückkanal
   * bliebe `initialSelection` in `App.tsx` dauerhaft gesetzt — ein SPÄTERER
   * manueller Wechsel auf "Verwalten" (nach Verlassen und Zurückkommen,
   * `ManagementView` wird dabei unmounted) würde denselben Server erneut
   * aufspringen lassen, obwohl der Nutzer längst etwas anderes wollte.
   * Einmalig aufgerufen, sobald `initialSelection` tatsächlich übernommen
   * wurde. */
  onInitialSelectionConsumed?: () => void;
}

/** Spec 0008, Abschnitt 6: Sidebar links, Formular im Hauptbereich. */
export function ManagementView({
  initialSelection = null,
  onInitialSelectionConsumed,
}: ManagementViewProps) {
  const [groups, setGroups] = useState<GroupDto[]>([]);
  const [servers, setServers] = useState<ServerDto[]>([]);
  const [selection, setSelection] = useState<Selection | null>(initialSelection);
  const [error, setError] = useState<string | null>(null);
  // Spec 0058, Teil 2: `true` genau dann, wenn die aktuelle `selection` aus
  // `initialSelection` (dem Navigations-Bus, "Mache ich selbst") stammt —
  // steuert `ServerForm`s `autoFocusNotes`. Auf `false` zurückgesetzt bei
  // jeder MANUELLEN Sidebar-Auswahl (`selectManually` unten), sonst würde
  // ein späterer normaler Klick auf einen ANDEREN Server fälschlich
  // ebenfalls automatisch scrollen/fokussieren.
  const [focusNotesOnOpen, setFocusNotesOnOpen] = useState(Boolean(initialSelection));

  // `initialSelection` kommt von außen (Navigations-Bus), kann sich also
  // ändern, NACHDEM diese Komponente bereits gemountet ist (anders als ein
  // normaler Initialwert) — deshalb zusätzlich per Effekt übernommen, nicht
  // nur als `useState`-Startwert.
  useEffect(() => {
    if (initialSelection) {
      setSelection(initialSelection);
      setFocusNotesOnOpen(true);
      onInitialSelectionConsumed?.();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialSelection]);

  const selectManually = (next: Selection | null) => {
    setFocusNotesOnOpen(false);
    setSelection(next);
  };

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
    selectManually(null);
    reload();
  };

  const handleCreated = () => {
    selectManually(null);
    reload();
  };

  return (
    <div className="flex min-h-0 flex-1">
      <Sidebar groups={groups} servers={servers} selection={selection} onSelect={selectManually} />
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
            autoFocusNotes={focusNotesOnOpen}
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
