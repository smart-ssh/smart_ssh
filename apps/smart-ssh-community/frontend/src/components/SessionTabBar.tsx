import { useTranslation } from "react-i18next";
import { sessionTabLabel, type SessionTab } from "../useSessionTabs";

const STATUS_DOT: Record<SessionTab["status"], string> = {
  connected: "bg-emerald-500",
  disconnected: "bg-slate-500",
  awaiting_host_key: "bg-amber-500",
};

interface SessionTabBarProps {
  tabs: SessionTab[];
  activeSessionId: string | null;
  onSwitch: (sessionId: string | null) => void;
  onRequestClose: (sessionId: string) => void;
}

/**
 * Spec 0017, Abschnitt 3: ein Tab pro offener Session (Servername,
 * bei MCP-Tabs "<Client> @ <Server>" samt MCP-Abzeichen, Spec 0103,
 * Statuspunkt, Schließen-Button), plus eine feste "Übersicht"-Kachel, die zu
 * den Server-/Verwaltungs-Screens zurückführt, ohne die im Hintergrund
 * offenen Sessions zu schließen — nur sichtbar, sobald mindestens ein Tab
 * offen ist (sonst zeigt `MainScreen` ohnehin schon dieselbe Übersicht).
 *
 * **Kein `data-tauri-drag-region`** auf den Tabs/Buttons selbst (Spec 0014,
 * Abschnitt 5 / Spec 0017, Abschnitt 3) — die bleiben klickbar statt das
 * Fenster zu ziehen. Das Attribut vererbt sich aber **nicht** automatisch
 * von `<AppHeader>` auf diesen Container (Tauri prüft pro Element, kein
 * Vererben über Nachfahren) — deshalb trägt der eigene Root-`<div>` hier
 * das Attribut erneut selbst (wie jede Zwischenebene in `AppHeader`), sonst
 * würde dieser volle, deckende Container die gesamte Fläche zum
 * "Loch" machen, sobald mindestens ein Tab existiert — nicht nur die
 * Buttons selbst.
 */
export function SessionTabBar({ tabs, activeSessionId, onSwitch, onRequestClose }: SessionTabBarProps) {
  const { t } = useTranslation();
  if (tabs.length === 0) return null;

  return (
    <div data-tauri-drag-region className="flex w-full items-center gap-1 overflow-x-auto">
      <button
        type="button"
        onClick={() => onSwitch(null)}
        title={t("sessionTabs.overview")}
        className={`font-heading shrink-0 border px-2.5 py-1 text-xs font-semibold tracking-wide ${
          activeSessionId === null
            ? "border-indigo-600/55 bg-indigo-600/16 text-indigo-400"
            : "border-transparent text-slate-400 hover:bg-slate-800"
        }`}
      >
        {t("sessionTabs.overview")}
      </button>

      {tabs.map((tab) => {
        const active = tab.sessionId === activeSessionId;
        // Spec 0103: MCP-Tabs heißen "<Client> @ <Server>", tragen ein
        // MCP-Abzeichen und zeigen eine wartende Bestätigung als
        // beschriftetes Abzeichen statt nur als Punkt — sie werden nie von
        // selbst aktiv, der Hinweis muss also aus dem Hintergrund auffallen.
        const label = sessionTabLabel(tab, t("sessionTabs.mcp.unnamedClient"));
        return (
          <div
            key={tab.sessionId}
            data-tauri-drag-region
            className={`group flex shrink-0 items-center gap-1.5 border px-2 py-1 text-xs ${
              active
                ? "border-indigo-600/55 bg-indigo-600/16 text-indigo-300"
                : "border-transparent text-slate-400 hover:bg-slate-800"
            }`}
          >
            <button
              type="button"
              onClick={() => onSwitch(tab.sessionId)}
              className="font-heading flex items-center gap-1.5 font-semibold tracking-wide"
              title={tab.mcp ? `${label} — ${t("sessionTabs.mcp.tabHint")}` : tab.serverName}
            >
              <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${STATUS_DOT[tab.status]}`} />
              {tab.mcp && (
                <span className="shrink-0 border border-fuchsia-500/60 px-1 text-[10px] leading-tight text-fuchsia-300">
                  {t("sessionTabs.mcp.badge")}
                </span>
              )}
              <span className={tab.mcp ? "max-w-[24ch] truncate" : "max-w-[12ch] truncate"}>
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
            </button>
            <button
              type="button"
              onClick={() => onRequestClose(tab.sessionId)}
              aria-label={t("sessionTabs.closeTab", { name: label })}
              className="shrink-0 px-0.5 text-slate-500 hover:text-slate-200"
            >
              ✕
            </button>
          </div>
        );
      })}
    </div>
  );
}
