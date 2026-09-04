import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commandErrorMessage,
  getMcpServerSettings,
  listServers,
  regenerateMcpServerToken,
  setMcpServerAllowedServers,
  setMcpServerConfirmTimeoutSecs,
  setMcpServerEnabled,
} from "../api";
import type { McpServerSettingsDto, ServerDto } from "../types";

/** Beispiel-Konfiguration für Claude Code (`.mcp.json`/`claude mcp add
 * --transport http`) — exakt das Format, das Claude Code für einen
 * HTTP-MCP-Server mit Bearer-Token erwartet. */
function exampleConfig(settings: McpServerSettingsDto): string {
  return JSON.stringify(
    {
      mcpServers: {
        "smart-ssh": {
          type: "http",
          url: settings.endpoint,
          headers: { Authorization: `Bearer ${settings.token}` },
        },
      },
    },
    null,
    2,
  );
}

/** Spec 0028, Abschnitt 9: eigener Abschnitt im Einstellungen-Screen
 * (`AiProviderSettings`) für den lokalen MCP-Server — standardmäßig
 * deaktiviert, Endpunkt/Token immer sichtbar (auch deaktiviert, damit sich
 * ein externer Client bereits vorkonfigurieren lässt), Server-Allow-Liste
 * per Mehrfachauswahl. Jede Änderung geht über einen Tauri-Command statt
 * direkt in den `tauri-plugin-store` (s. `crate::mcp_settings`-Moduldoc):
 * Aktivieren/Token-Rotation/Allow-Liste haben eine sofortige Live-Wirkung
 * auf einen ggf. bereits laufenden Server.
 */
export function McpServerSettings() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<McpServerSettingsDto | null>(null);
  const [servers, setServers] = useState<ServerDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    Promise.all([getMcpServerSettings(), listServers()])
      .then(([loadedSettings, loadedServers]) => {
        setSettings(loadedSettings);
        setServers(loadedServers);
      })
      .catch((err) => setError(commandErrorMessage(err)));
  }, []);

  const handleToggleEnabled = async (enabled: boolean) => {
    setBusy(true);
    setError(null);
    try {
      setSettings(await setMcpServerEnabled(enabled));
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  /** Spec 0028, Abschnitt 9: "invalidiert das alte Token sofort" — eine
   * Rückfrage, weil das jede bereits konfigurierte externe Client-
   * Verbindung (z. B. Claude Code) ab sofort ablehnt, bis der neue Wert
   * dort ebenfalls eingetragen wird. */
  const handleRegenerateToken = async () => {
    if (!window.confirm(t("mcpServer.regenerateConfirm"))) return;
    setBusy(true);
    setError(null);
    try {
      setSettings(await regenerateMcpServerToken());
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  /** Spec 0028, Abschnitt 7: das Timeout wird als volle Minuten im UI
   * eingegeben, aber sekundengenau gespeichert/übertragen (Backend-Default
   * 300s = 5 Minuten) — Minuten sind die für den Nutzer sinnvolle Einheit,
   * ohne die Backend-Präzision künstlich einzuschränken. */
  const handleChangeConfirmTimeoutMinutes = async (minutes: number) => {
    if (!Number.isFinite(minutes) || minutes < 1) return;
    setBusy(true);
    setError(null);
    try {
      setSettings(await setMcpServerConfirmTimeoutSecs(Math.round(minutes * 60)));
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const handleToggleAllowedServer = async (serverId: string, checked: boolean) => {
    if (!settings) return;
    const next = checked
      ? [...settings.allowedServerIds, serverId]
      : settings.allowedServerIds.filter((id) => id !== serverId);
    setBusy(true);
    setError(null);
    try {
      setSettings(await setMcpServerAllowedServers(next));
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mb-6 border-t border-slate-700 pt-4">
      <h3 className="font-heading mb-2 text-sm font-semibold tracking-wide text-slate-200">
        {t("mcpServer.title")}
      </h3>
      <p className="mb-2 text-xs text-slate-500">{t("mcpServer.hint")}</p>

      {error && <p className="mb-2 rounded bg-red-950 px-2 py-1 text-xs text-red-300">{error}</p>}

      {settings && (
        <>
          <label className="mb-2 flex items-center gap-2 text-sm text-slate-300">
            <input
              type="checkbox"
              checked={settings.enabled}
              disabled={busy}
              onChange={(e) => handleToggleEnabled(e.target.checked)}
            />
            {t("mcpServer.enable")}
          </label>

          <div className="mb-3 space-y-2 rounded border border-slate-700 bg-slate-900 p-3">
            <div>
              <p className="text-xs text-slate-400">{t("mcpServer.endpointLabel")}</p>
              <code className="block text-xs break-all text-slate-200 select-all">
                {settings.endpoint}
              </code>
            </div>
            <div>
              <p className="text-xs text-slate-400">{t("mcpServer.tokenLabel")}</p>
              <code className="block text-xs break-all text-slate-200 select-all">
                {settings.token}
              </code>
            </div>
            <button
              type="button"
              onClick={handleRegenerateToken}
              disabled={busy}
              className="rounded border border-slate-600 px-2 py-1 text-xs text-slate-300 hover:bg-slate-700 disabled:opacity-50"
            >
              {t("mcpServer.regenerateToken")}
            </button>
          </div>

          <div className="mb-3">
            <label className="mb-1 flex items-center gap-2 text-xs text-slate-400">
              {t("mcpServer.confirmTimeoutLabel")}
              <input
                type="number"
                min={1}
                step={1}
                defaultValue={Math.round(settings.confirmTimeoutSecs / 60)}
                key={settings.confirmTimeoutSecs}
                disabled={busy}
                onBlur={(e) => handleChangeConfirmTimeoutMinutes(e.target.valueAsNumber)}
                className="w-16 rounded border border-slate-600 bg-slate-900 px-1.5 py-0.5 text-sm text-slate-200"
              />
              {t("mcpServer.confirmTimeoutUnit")}
            </label>
            <p className="text-xs text-slate-500">{t("mcpServer.confirmTimeoutHint")}</p>
          </div>

          <div className="mb-3">
            <p className="mb-1 text-xs font-semibold text-slate-400">
              {t("mcpServer.allowListLabel")}
            </p>
            {servers.length === 0 ? (
              <p className="text-xs text-slate-500">{t("mcpServer.noServers")}</p>
            ) : (
              <ul className="max-h-40 divide-y divide-slate-700 overflow-y-auto rounded border border-slate-700">
                {servers.map((server) => (
                  <li key={server.id} className="px-2 py-1.5">
                    <label className="flex items-center gap-2 text-sm text-slate-300">
                      <input
                        type="checkbox"
                        checked={settings.allowedServerIds.includes(server.id)}
                        disabled={busy}
                        onChange={(e) => handleToggleAllowedServer(server.id, e.target.checked)}
                      />
                      {server.name}
                    </label>
                  </li>
                ))}
              </ul>
            )}
          </div>

          <p className="mb-1 text-xs text-slate-500">{t("mcpServer.exampleConfigHint")}</p>
          <pre className="max-h-40 overflow-auto rounded border border-slate-700 bg-slate-950 p-2 text-xs whitespace-pre-wrap text-slate-300">
            {exampleConfig(settings)}
          </pre>
        </>
      )}
    </div>
  );
}
