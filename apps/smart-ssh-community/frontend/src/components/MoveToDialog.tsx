// Issue #582, Spec 0113 §5a: "Move to…" for a server or folder in the narrow
// layout — a non-drag way to do what drag and drop does. The chosen target
// goes through the same `onMove` (→ `performMove`) as a drop.
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { buildGroupTree, type GroupTreeNode } from "../lib/groupTree";
import { classifyDrop, type DragItem, type DropTarget } from "../lib/treeDrag";
import type { GroupDto, ServerDto } from "../api/types";
import { Button, DialogFrame, Icon } from "./ui";

interface Props {
  item: DragItem;
  groups: GroupDto[];
  servers: ServerDto[];
  onMove: (item: DragItem, target: DropTarget) => void;
  /** Spec 0103 §3a: set for a copy entry — offers "Keep as regular entry". */
  onKeepAsRegular?: () => void;
  onClose: () => void;
}

function flatten(nodes: GroupTreeNode[], depth: number, out: { group: GroupDto; depth: number }[]) {
  for (const n of nodes) {
    out.push({ group: n.group, depth });
    flatten(n.children, depth + 1, out);
  }
  return out;
}

export function MoveToDialog({ item, groups, servers, onMove, onKeepAsRegular, onClose }: Props) {
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
    "flex min-h-11 w-full items-center gap-2 px-3 py-2 text-left text-sm text-fg hover:bg-surface-raised focus-ring-flush disabled:opacity-40 disabled:hover:bg-transparent";
  const disabledFor = (target: DropTarget) => classifyDrop(item, target, groups, servers) !== "valid";
  // The item's present location: its server's group, or a group's parent.
  const currentParentId =
    item.kind === "server"
      ? (servers.find((s) => s.id === item.id)?.groupId ?? null)
      : (groups.find((g) => g.id === item.id)?.parentId ?? null);
  const currentMarker = (groupId: string | null) =>
    groupId === currentParentId ? (
      <span className="ml-auto shrink-0 font-mono text-xs text-fg-subtle">{t("treeDrag.current")}</span>
    ) : null;
  return (
    <DialogFrame
      layer="app"
      icon={<Icon name="move" />}
      title={<span className="break-words">{t("treeDrag.moveTo", { name: item.label })}</span>}
      onClose={onClose}
      actions={<Button onClick={onClose}>{t("common.cancel")}</Button>}
    >
      {onKeepAsRegular && (
        <Button
          variant="bare"
          type="button"
          className={`${optionClass} mb-2 border border-border`}
          onClick={() => {
            onClose();
            onKeepAsRegular();
          }}
        >
          {t("copyMarker.keep")}
        </Button>
      )}
      <ul className="divide-y divide-surface-raised rounded border border-border bg-surface-sunken">
        <li>
          <Button variant="bare" type="button" className={optionClass} disabled={disabledFor({ kind: "root" })} onClick={() => choose({ kind: "root" })}>
            <Icon name="root" className="shrink-0 text-fg-subtle" />
            <span className="min-w-0 break-words">{t("treeDrag.hintRoot")}</span>
            {currentMarker(null)}
          </Button>
        </li>
        {entries.map(({ group, depth }) => (
          <li key={group.id}>
            <Button variant="bare"
              type="button"
              className={optionClass}
              style={{ paddingLeft: "12px" }}
              disabled={disabledFor({ kind: "group", id: group.id })}
              onClick={() => choose({ kind: "group", id: group.id })}
            >
              {/* Tree connector lines: one guide per ancestor level, an elbow at the row's own level. */}
              {Array.from({ length: depth }, (_, i) => (
                <span
                  key={i}
                  aria-hidden="true"
                  data-testid="move-connector"
                  className={`-my-2 self-stretch w-4 shrink-0 border-l border-border-strong ${i === depth - 1 ? "relative after:absolute after:left-0 after:top-1/2 after:w-3 after:border-t after:border-border-strong" : ""}`}
                />
              ))}
              <Icon name="folder" className="shrink-0" />
              <span className="min-w-0 break-words">{group.name}</span>
              {currentMarker(group.id)}
            </Button>
          </li>
        ))}
      </ul>
    </DialogFrame>
  );
}
