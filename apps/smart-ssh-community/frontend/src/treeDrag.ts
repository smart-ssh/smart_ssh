// Issue #48 / Spec 0103: reine Logik für das Verschieben von Servern und
// Gruppen per Drag-and-drop in beiden Bäumen (Verbinden-Liste und
// Verwalten-Sidebar). Bewusst getrennt von der Zeiger-Mechanik
// (`useTreeDrag.ts`), damit die Regeln „wohin darf was" ohne DOM testbar
// sind.

import { moveGroup, moveServerToGroup } from "./api";
import type { GroupDto, ServerDto } from "./types";

/** Was gezogen wird. Der lokale Pseudo-Server (Spec 0032) wird nie zu
 * einem `DragItem` — die Bäume vergeben für ihn keine Zieh-Handler. */
export type DragItem =
  | { kind: "server"; id: string; label: string }
  | { kind: "group"; id: string; label: string };

/** Wohin gezogen wird: eine Gruppe oder die Wurzelebene („Ohne Gruppe"). */
export type DropTarget = { kind: "group"; id: string } | { kind: "root" };

/**
 * - `valid`: ein echtes Verschieben.
 * - `noop`: bliebe unverändert (schon dort, oder eine Gruppe auf sich
 *   selbst) — kein Hervorheben, kein Aufruf.
 * - `cycle`: eine Gruppe in ihren eigenen Nachfahren — wird rot
 *   hervorgehoben und beim Loslassen an das Backend gegeben, das sie mit
 *   `GROUP_CYCLE_DETECTED` ablehnt (dieselbe Prüfung wie im Formular,
 *   `validate_no_cycle`); die Meldung wird sichtbar angezeigt.
 */
export type DropKind = "valid" | "noop" | "cycle";

/** Attribut, mit dem ein Element sich als Ablageziel ausweist. Werte:
 * `root`, `group:<id>` oder `none` (ausdrücklich kein Ziel, etwa der
 * angeheftete lokale Server innerhalb eines Wurzel-Bereichs). */
export const DROP_TARGET_ATTRIBUTE = "data-drop-target";

export function groupDropTargetValue(groupId: string): string {
  return `group:${groupId}`;
}

export function parseDropTarget(value: string | null | undefined): DropTarget | null {
  if (value === "root") return { kind: "root" };
  if (value?.startsWith("group:")) {
    const id = value.slice("group:".length);
    return id ? { kind: "group", id } : null;
  }
  return null;
}

/** `true`, wenn `candidateId` die Gruppe `ancestorId` selbst oder einer
 * ihrer Nachfahren ist. Läuft die `parentId`-Kette von `candidateId`
 * aufwärts und bricht bei einem (eigentlich unmöglichen) Zyklus in den
 * Daten ab, statt endlos zu laufen. */
function isSelfOrDescendant(groups: GroupDto[], ancestorId: string, candidateId: string): boolean {
  const byId = new Map(groups.map((g) => [g.id, g]));
  const visited = new Set<string>();
  let current: string | null = candidateId;
  while (current !== null && !visited.has(current)) {
    if (current === ancestorId) return true;
    visited.add(current);
    current = byId.get(current)?.parentId ?? null;
  }
  return false;
}

export function classifyDrop(
  item: DragItem,
  target: DropTarget,
  groups: GroupDto[],
  servers: ServerDto[],
): DropKind {
  const targetGroupId = target.kind === "group" ? target.id : null;
  if (item.kind === "server") {
    const server = servers.find((s) => s.id === item.id);
    if (!server || server.isLocal) return "noop";
    return server.groupId === targetGroupId ? "noop" : "valid";
  }
  const group = groups.find((g) => g.id === item.id);
  if (!group) return "noop";
  if (targetGroupId === group.id) return "noop";
  if (group.parentId === targetGroupId) return "noop";
  if (targetGroupId !== null && isSelfOrDescendant(groups, group.id, targetGroupId)) {
    return "cycle";
  }
  return "valid";
}

/** Tailwind-Klassen für ein Ablageziel, je nachdem, was ein Loslassen dort
 * bewirken würde. */
export function dropHighlightClass(kind: DropKind | null): string {
  if (kind === "valid") return "bg-indigo-900/40 ring-2 ring-inset ring-indigo-500";
  if (kind === "cycle") return "bg-red-950/40 ring-2 ring-inset ring-red-500";
  return "";
}

/** Führt das Verschieben über die schmalen Backend-Befehle aus (nie über
 * `update_server` mit vollständigem `ServerInput`). Wirft den
 * Backend-Fehler weiter — der Aufrufer zeigt ihn an. */
export async function performMove(item: DragItem, target: DropTarget): Promise<void> {
  const targetGroupId = target.kind === "group" ? target.id : null;
  if (item.kind === "server") {
    await moveServerToGroup(item.id, targetGroupId);
  } else {
    await moveGroup(item.id, targetGroupId);
  }
}
