import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode, commandErrorMessage, listGroups, listServers } from "../api";
import { translateErrorCode } from "../errorCodes";
import { performMove, type DragItem, type DropTarget } from "../treeDrag";
import type { GroupDto, ServerDto } from "../types";
import { GroupForm, type MovedTo } from "./GroupForm";
import { ServerForm } from "./ServerForm";
import { Sidebar, type Selection } from "./Sidebar";
import { SshConfigExportDialog } from "./SshConfigExportDialog";
import { SshConfigImportDialog } from "./SshConfigImportDialog";

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
  const { t } = useTranslation();
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
  // Spec 0075: Import-Vorschau-Dialog bzw. Export-Ergebnis-Dialog. Jeweils
  // nur eines gleichzeitig sichtbar — beide öffnen einen nativen
  // Dateidialog im Backend, ein zweiter gleichzeitig wäre verwirrend.
  const [importOpen, setImportOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);

  // `initialSelection` kommt von außen (Navigations-Bus), kann sich also
  // ändern, NACHDEM diese Komponente bereits gemountet ist (anders als ein
  // normaler Initialwert) — deshalb zusätzlich per Effekt übernommen, nicht
  // nur als `useState`-Startwert.
  useEffect(() => {
    if (initialSelection) {
      setSelection(initialSelection);
      setFocusNotesOnOpen(true);
      setMovedTo(null);
      setNewFormRevision((n) => n + 1);
      onInitialSelectionConsumed?.();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialSelection]);

  // Issue #49: erhöht bei jeder Auswahl (auch einem erneuten Klick auf
  // „+ Server"/„+ Gruppe" mit derselben Vorgabe) — Teil des `key` der
  // Neu-Formulare. So beginnt jedes „+" ein frisches Formular, während ein
  // offenes Formular sonst (z. B. beim Neuladen der Listen nach einem
  // Verschieben) seine Eingaben behält.
  const [newFormRevision, setNewFormRevision] = useState(0);

  const selectManually = (next: Selection | null) => {
    setFocusNotesOnOpen(false);
    setMovedTo(null);
    setSelection(next);
    setNewFormRevision((n) => n + 1);
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

  // Issue #63: neuer Ort des gerade geöffneten Elements nach einem
  // erfolgreichen Verschieben per Drag-and-drop. `ServerForm`/`GroupForm`
  // übernehmen daraus nur das Gruppen- bzw. Übergruppen-Feld, alle übrigen
  // ungespeicherten Eingaben bleiben stehen (kein Remount mehr, s. Issue
  // #48). Ohne diese Übernahme stünde im Formular noch die alte Gruppe,
  // und ein späteres Speichern würde das Verschieben still rückgängig
  // machen. Jedes Verschieben erzeugt ein neues Objekt, damit das Formular
  // auch einen erneuten Wechsel zurück auf denselben Ort bemerkt. Bei
  // jedem Auswahlwechsel zurückgesetzt (`selectManually`).
  const [movedTo, setMovedTo] = useState<MovedTo | null>(null);

  /** Issue #48 / Spec 0103: Verschieben per Drag-and-drop aus der Sidebar.
   * Ein Zyklus wird vom Backend abgelehnt; die übersetzte Meldung
   * erscheint im Fehlerbereich, geändert wird nichts. */
  const handleMove = async (item: DragItem, target: DropTarget) => {
    setError(null);
    try {
      await performMove(item, target);
      if (selection && "id" in selection && selection.kind === item.kind && selection.id === item.id) {
        setMovedTo({ groupId: target.kind === "group" ? target.id : null });
      }
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    }
    reload();
  };

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
      <Sidebar
        groups={groups}
        servers={servers}
        selection={selection}
        onSelect={selectManually}
        onImportSshConfig={() => setImportOpen(true)}
        onExportSshConfig={() => setExportOpen(true)}
        onMove={(item, target) => void handleMove(item, target)}
      />
      {importOpen && (
        <SshConfigImportDialog
          onClose={() => setImportOpen(false)}
          onImported={() => {
            reload();
            selectManually(null);
          }}
        />
      )}
      {exportOpen && <SshConfigExportDialog onClose={() => setExportOpen(false)} />}
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
         * server form". Ein Verschieben des geöffneten Elements per
         * Drag-and-drop remountet bewusst NICHT (Issue #63), sondern
         * reicht nur den neuen Ort über `movedTo` durch. */}
        {selection?.kind === "group" && (
          <GroupForm
            key={selection.id}
            groupId={selection.id}
            movedTo={movedTo}
            defaultParentId={null}
            allGroups={groups}
            onSaved={reload}
            onDeleted={handleDeleted}
          />
        )}
        {selection?.kind === "newGroup" && (
          <GroupForm
            key={`new-group-${newFormRevision}`}
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
            movedTo={movedTo}
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
            key={`new-server-${newFormRevision}`}
            serverId={null}
            defaultGroupId={selection.groupId}
            allGroups={groups}
            allServers={servers}
            onSaved={handleCreated}
            onDeleted={handleDeleted}
          />
        )}
        {!selection && (
          // Spec 0072, Teil 3 (BL-0202): dieser Platzhaltertext stand fest
          // deutsch da und erschien so auch in der englischen Oberfläche —
          // lag außerhalb des Dateisatzes von Spec 0069. Interpoliert die
          // Button-Beschriftungen aus `sidebar.addGroup`/`sidebar.addServer`
          // statt sie hier ein zweites Mal wörtlich zu führen.
          <p className="p-4 text-sm text-slate-400">
            {t("management.emptyState", {
              addGroup: t("sidebar.addGroup"),
              addServer: t("sidebar.addServer"),
            })}
          </p>
        )}
      </div>
    </div>
  );
}
