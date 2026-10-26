// Issue #582, Spec 0113 §5a: "Move to…" for a server or folder in the narrow
// layout — a non-drag way to do what drag and drop does. The chosen target
// goes through the same `onMove` (→ `performMove`) as a drop.
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { buildGroupTree, type GroupTreeNode } from "../groupTree";
import { classifyDrop, type DragItem, type DropTarget } from "../treeDrag";
import type { GroupDto, ServerDto } from "../types";
import { Button, DialogFrame } from "./ui";

interface Props {
  item: DragItem;
  groups: GroupDto[];
  servers: ServerDto[];
  onMove: (item: DragItem, target: DropTarget) => void;
  onClose: () => void;
}

function flatten(nodes: GroupTreeNode[], depth: number, out: { group: GroupDto; depth: number }[]) {
  for (const n of nodes) {
    out.push({ group: n.group, depth });
    flatten(n.children, depth + 1, out);
  }
  return out;
}

export function MoveToDialog({ item, groups, servers, onMove, onClose }: Props) {
  const { t } = useTranslation();
  const entries = useMemo(
    () => flatten(buildGroupTree(groups, servers, []).roots, 0, []),
    [groups, servers],
  );
  const choose = (target: DropTarget) => {
    onClose();
    onMove(item, target);
  };
  const optionClass =
    "flex min-h-11 w-full items-center rounded px-3 py-2 text-left text-sm text-fg hover:bg-surface-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg disabled:opacity-40";
  const disabledFor = (target: DropTarget) => classifyDrop(item, target, groups, servers) !== "valid";
  return (
    <DialogFrame
      layer="app"
      title={<span className="break-words">{t("treeDrag.moveTo", { name: item.label })}</span>}
      onClose={onClose}
      actions={<Button onClick={onClose}>{t("common.cancel")}</Button>}
    >
      <ul className="divide-y divide-surface-raised rounded border border-border">
        <li>
          <button type="button" className={optionClass} disabled={disabledFor({ kind: "root" })} onClick={() => choose({ kind: "root" })}>
            {t("mainScreen.ungrouped")}
          </button>
        </li>
        {entries.map(({ group, depth }) => (
          <li key={group.id}>
            <button
              type="button"
              className={optionClass}
              style={{ paddingLeft: `${depth * 16 + 12}px` }}
              disabled={disabledFor({ kind: "group", id: group.id })}
              onClick={() => choose({ kind: "group", id: group.id })}
            >
              <span className="min-w-0 break-words">📁 {group.name}</span>
            </button>
          </li>
        ))}
      </ul>
    </DialogFrame>
  );
}
