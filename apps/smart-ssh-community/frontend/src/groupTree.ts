// Spec 0033, Abschnitt 5: Baum-Aufbau aus `list_groups()`/`list_servers()`
// als eigene, reine Funktion — vorher separat inline im Sidebar (Spec 0008
// Abschnitt 6). Jetzt einzige Implementierung, von `Sidebar` (Verwalten-Tab)
// UND der neuen gruppierten Hauptübersicht (Spec 0033 Abschnitt 3)
// gleichermaßen genutzt, statt zweier leicht unterschiedlicher
// Nachbauten. Der lokale Pseudo-Server (Spec 0032) wird hier bewusst NICHT
// mit aufgenommen — er gehört nie einer Gruppe an und wird von den
// Aufrufern selbst separat/angeheftet dargestellt.

import type { GroupDto, ServerDto, UnusableServerDto } from "./types";

export interface GroupTreeNode {
  group: GroupDto;
  /** Direkt dieser Gruppe zugeordnete Server (nicht die der Untergruppen). */
  servers: ServerDto[];
  /** Issue #100: direkt dieser Gruppe zugeordnete, nicht nutzbare Server
   * (Anmeldeart unlesbar). */
  unusableServers: UnusableServerDto[];
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
  /** Issue #100: nicht nutzbare Server ohne Gruppe. */
  ungroupedUnusableServers: UnusableServerDto[];
}

function buildNode(
  group: GroupDto,
  groups: GroupDto[],
  servers: ServerDto[],
  unusable: UnusableServerDto[],
): GroupTreeNode {
  return {
    group,
    servers: servers.filter((s) => s.groupId === group.id),
    unusableServers: unusable.filter((s) => s.groupId === group.id),
    // Bewusst KEINE Filterung "nur wenn Server enthalten" — eine leere
    // Gruppe mit einer Untergruppe, die selbst Server enthält, muss
    // trotzdem erscheinen (Spec 0033, Abschnitt 4), sonst wäre die
    // Hierarchie für die Untergruppe nicht mehr nachvollziehbar. Reine
    // Rekursion ohne Filterung erfüllt das automatisch.
    children: groups
      .filter((g) => g.parentId === group.id)
      .map((g) => buildNode(g, groups, servers, unusable)),
  };
}

/** Issue #100: `unusable` sind die nicht nutzbaren Server — sie hängen an
 * derselben Stelle im Baum wie ein normaler Server ihrer Gruppe, bleiben
 * aber eine eigene Liste, damit kein Aufrufer sie versehentlich wie einen
 * verbindbaren Server behandelt. */
export function buildGroupTree(
  groups: GroupDto[],
  servers: ServerDto[],
  unusable: UnusableServerDto[] = [],
): GroupTree {
  const roots = groups
    .filter((g) => g.parentId === null)
    .map((g) => buildNode(g, groups, servers, unusable));
  const ungroupedServers = servers.filter((s) => s.groupId === null && !s.isLocal);
  const ungroupedUnusableServers = unusable.filter((s) => s.groupId === null);
  return { roots, ungroupedServers, ungroupedUnusableServers };
}

/** Issue #49: ein Eintrag der Gruppen-Auswahl in `ServerForm`/`GroupForm`. */
export interface GroupOption {
  group: GroupDto;
  /** Verschachtelungstiefe, 0 = Wurzelebene. */
  depth: number;
  /** Voller Pfad von der Wurzel, z. B. `Prod / Web`. */
  path: string;
}

/** Issue #49: die Gruppen-Dropdowns zeigten nur eine flache Liste der
 * Namen — eine vorbelegte Untergruppe war darin nicht von einer
 * gleichnamigen anderswo zu unterscheiden. Liefert die Gruppen in
 * Baum-Reihenfolge (Tiefensuche, Geschwister in Eingabe-Reihenfolge) mit
 * Tiefe und vollem Pfad. Anders als `buildGroupTree` geht hier keine
 * Gruppe verloren: eine Gruppe, deren Elterngruppe fehlt, oder ein
 * (eigentlich vom Backend verhinderter) Zyklus erscheint auf der
 * Wurzelebene statt aus der Auswahl zu verschwinden. */
export function flattenGroupOptions(groups: GroupDto[]): GroupOption[] {
  const ids = new Set(groups.map((g) => g.id));
  const visited = new Set<string>();
  const result: GroupOption[] = [];
  const visit = (group: GroupDto, depth: number, parentPath: string | null) => {
    if (visited.has(group.id)) return;
    visited.add(group.id);
    const path = parentPath === null ? group.name : `${parentPath} / ${group.name}`;
    result.push({ group, depth, path });
    for (const child of groups) {
      if (child.parentId === group.id) visit(child, depth + 1, path);
    }
  };
  for (const g of groups) {
    if (g.parentId === null || !ids.has(g.parentId)) visit(g, 0, null);
  }
  for (const g of groups) visit(g, 0, null);
  return result;
}

/** Issue #49: Beschriftung einer Gruppen-Option — eingerückter voller
 * Pfad. Geschützte Leerzeichen, weil normale in einem `<option>`
 * zusammengefasst würden. */
export function groupOptionLabel(option: GroupOption): string {
  return `${"  ".repeat(option.depth)}${option.path}`;
}
