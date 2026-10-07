import { useTranslation } from "react-i18next";
import type { DropKind } from "../treeDrag";
import type { TreeDragState } from "../useTreeDrag";

/** Issue #48 / Spec 0103: Beschriftung, die beim Ziehen dem Zeiger folgt —
 * was gezogen wird und was ein Loslassen hier bewirkt. `pointer-events:
 * none`, damit `elementFromPoint` durch sie hindurch das Ziel darunter
 * findet. */
export function TreeDragGhost({ drag, kind }: { drag: TreeDragState; kind: DropKind | null }) {
  const { t } = useTranslation();
  const icon = drag.item.kind === "group" ? "📁" : "🖥️";
  const hint =
    kind === "valid"
      ? drag.target?.kind === "root"
        ? t("treeDrag.hintRoot")
        : t("treeDrag.hintGroup")
      : kind === "cycle"
        ? t("treeDrag.hintCycle")
        : null;
  return (
    <div
      aria-hidden="true"
      style={{ position: "fixed", left: drag.x + 14, top: drag.y + 14 }}
      className={`pointer-events-none z-50 rounded border px-2 py-1 text-xs shadow-lg ${
        kind === "cycle"
          ? "border-red-500 bg-red-950 text-red-100"
          : "border-slate-600 bg-slate-800 text-slate-100"
      }`}
    >
      {icon} {drag.item.label}
      {hint && <span className="ml-2 text-slate-400">{hint}</span>}
    </div>
  );
}
