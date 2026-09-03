// Spec 0033, Abschnitt 5: Baum-Aufbau aus `list_groups()`/`list_servers()`
// als eigene, reine Funktion — vorher separat inline im Sidebar (Spec 0008
// Abschnitt 6). Jetzt einzige Implementierung, von `Sidebar` (Verwalten-Tab)
// UND der neuen gruppierten Hauptübersicht (Spec 0033 Abschnitt 3)
// gleichermaßen genutzt, statt zweier leicht unterschiedlicher
// Nachbauten. Der lokale Pseudo-Server (Spec 0032) wird hier bewusst NICHT
// mit aufgenommen — er gehört nie einer Gruppe an und wird von den
// Aufrufern selbst separat/angeheftet dargestellt.

import type { GroupDto, ServerDto } from "./types";

export interface GroupTreeNode {
  group: GroupDto;
  /** Direkt dieser Gruppe zugeordnete Server (nicht die der Untergruppen). */
  servers: ServerDto[];
  children: GroupTreeNode[];
}

export interface GroupTree {
  /** Gruppen ohne `parentId` (Wurzelebene). */
  roots: GroupTreeNode[];
  /** Server ohne `groupId` ("Ohne Gruppe", Spec 0033 Abschnitt 3). Der
   * lokale Pseudo-Server (`isLocal: true`) wird hier explizit
   * ausgeschlossen, obwohl er ebenfalls `groupId: null` hat — er ist kein
   * normaler ungruppierter Server, sondern wird von den Aufrufern separat
   * angeheftet dargestellt (Spec 0032 Abschnitt 5 / Spec 0033 Abschnitt 3). */
  ungroupedServers: ServerDto[];
}

function buildNode(group: GroupDto, groups: GroupDto[], servers: ServerDto[]): GroupTreeNode {
  return {
    group,
    servers: servers.filter((s) => s.groupId === group.id),
    // Bewusst KEINE Filterung "nur wenn Server enthalten" — eine leere
    // Gruppe mit einer Untergruppe, die selbst Server enthält, muss
    // trotzdem erscheinen (Spec 0033, Abschnitt 4), sonst wäre die
    // Hierarchie für die Untergruppe nicht mehr nachvollziehbar. Reine
    // Rekursion ohne Filterung erfüllt das automatisch.
    children: groups
      .filter((g) => g.parentId === group.id)
      .map((g) => buildNode(g, groups, servers)),
  };
}

export function buildGroupTree(groups: GroupDto[], servers: ServerDto[]): GroupTree {
  const roots = groups.filter((g) => g.parentId === null).map((g) => buildNode(g, groups, servers));
  const ungroupedServers = servers.filter((s) => s.groupId === null && !s.isLocal);
  return { roots, ungroupedServers };
}
