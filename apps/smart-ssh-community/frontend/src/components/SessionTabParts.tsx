// Parts of a session tab shared by the wide tab bar and the narrow burger
// menu (issue #579), so label, state indicator and badges stay identical.
import type { TFunction } from "i18next";
import { Spinner } from "./ui";
import { sessionTabLabel, type SessionTab } from "../useSessionTabs";

const STATUS_DOT: Record<SessionTab["status"], string> = {
  connected: "bg-emerald-500",
  disconnected: "bg-slate-500",
  reconnecting: "bg-amber-500",
  awaiting_host_key: "bg-amber-500",
};

/** The visible label of a tab, including connecting/failed wording. */
export function tabDisplayLabel(tab: SessionTab, t: TFunction): string {
  return tab.phase === "connecting"
    ? t("connectTab.connecting", { name: tab.serverName })
    : tab.phase === "failed"
      ? t("connectTab.failed", { name: tab.serverName })
      : sessionTabLabel(tab, t("sessionTabs.mcp.unnamedClient"));
}

/** State indicator: spinner while connecting, cross when failed, else a dot. */
export function TabStateIndicator({ tab }: { tab: SessionTab }) {
  return tab.phase === "connecting" ? (
    <Spinner className="h-2.5 w-2.5 shrink-0" />
  ) : tab.phase === "failed" ? (
    <span aria-hidden="true" className="shrink-0 text-red-400">
      ✗
    </span>
  ) : (
    <span aria-hidden="true" className={`h-1.5 w-1.5 shrink-0 rounded-full ${STATUS_DOT[tab.status]}`} />
  );
}

/** Indicator, MCP badge, label and pending-confirmation marker of one tab. */
export function SessionTabContent({
  tab,
  label,
  t,
  labelClass,
}: {
  tab: SessionTab;
  label: string;
  t: TFunction;
  labelClass: string;
}) {
  return (
    <>
      <TabStateIndicator tab={tab} />
      {tab.mcp && (
        <span className="shrink-0 border border-fuchsia-500/60 px-1 text-[10px] leading-tight text-fuchsia-300">
          {t("sessionTabs.mcp.badge")}
        </span>
      )}
      <span title={label} className={labelClass}>
        {label}
      </span>
      {tab.hasPendingAction &&
        (tab.mcp ? (
          <span className="shrink-0 animate-pulse bg-amber-400 px-1 text-[10px] leading-tight text-slate-950">
            {t("sessionTabs.mcp.pendingBadge")}
          </span>
        ) : (
          <span
            className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-amber-400"
            title={t("sessionTabs.pendingAction")}
          />
        ))}
    </>
  );
}
