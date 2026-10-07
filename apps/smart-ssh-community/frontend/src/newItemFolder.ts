// Issue #49: „+ Server"/„+ Gruppe" in der Verwalten-Sidebar legen das neue
// Element in der gerade aktuellen Gruppe an. Reine Funktion, damit die
// Regel ohne Rendering testbar ist.

import type { Selection } from "./components/Sidebar";
import type { ServerDto } from "./types";

/** Die Gruppe, die ein neuer Server bzw. eine neue Gruppe als Vorgabe
 * bekommt:
 * - ausgewählte Gruppe → diese Gruppe,
 * - ausgewählter Server → seine Gruppe (ungruppiert/lokal → keine),
 * - offenes Neu-Formular → dessen Vorgabe (ein zweiter Klick auf „+"
 *   bleibt in derselben Gruppe),
 * - nichts ausgewählt → keine.
 *
 * Nur eine Vorgabe: das Formular lässt die Gruppe vor dem Speichern
 * ändern. */
export function folderForNewItem(selection: Selection | null, servers: ServerDto[]): string | null {
  if (!selection) return null;
  switch (selection.kind) {
    case "group":
      return selection.id;
    case "server":
      return servers.find((s) => s.id === selection.id)?.groupId ?? null;
    case "newGroup":
      return selection.parentId;
    case "newServer":
      return selection.groupId;
  }
}
