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
import { Button, FIELD_CONTROL_CLASS } from "./ui";
import type { McpServerSettingsDto, ServerDto } from "../types";

/** Beispiel-Konfiguration für Claude Code (`.mcp.json`/`claude mcp add
 * --transport http`) — exakt das Format, das Claude Code für einen
 * HTTP-MCP-Server mit Bearer-Token erwartet. */
function exampleConfig(settings: McpServerSettingsDto, token: string = settings.token): string {
  return JSON.stringify(
    {
      mcpServers: {
        "smart-ssh": {
          type: "http",
          url: settings.endpoint,
          headers: { Authorization: `Bearer ${token}` },
        },
      },
    },
    null,
    2,
  );
}

/** Fixed-length placeholder: never derived from the real token, so neither
 * its text nor its length leaks while the token is hidden (Spec 0028). */
const MASKED_TOKEN = "••••••••••••••••";

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
  const [tokenRevealed, setTokenRevealed] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"copied" | "failed" | null>(null);

  useEffect(() => {
    Promise.all([getMcpServerSettings(), listServers()])
      .then(([loadedSettings, loadedServers]) => {
        setSettings(loadedSettings);
        setServers(loadedServers);
      })
      .catch((err) => setError(commandErrorMessage(err)));
  }, []);

  /** Kopiert immer die vollständige Konfiguration mit dem echten Token,
   * unabhängig vom Anzeigezustand (nie loggen). */
  const handleCopyConfig = async () => {
    if (!settings) return;
    try {
      await navigator.clipboard.writeText(exampleConfig(settings));
      setCopyStatus("copied");
    } catch {
      setCopyStatus("failed");
    }
  };

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

  // Spec 0055, Teil 3: `SettingsScreen` rendert den Sektions-Titel bereits
  // selbst — s. `ChatRetentionSettings.tsx`s identischer Fund/Begründung
  // (auch für den hier ebenfalls entfernten `border-t`/`pt-4`-Rahmen).
  return (
    <div>
      <p className="mb-2 text-xs text-fg-subtle">{t("mcpServer.hint")}</p>

      {error && <p className="mb-2 rounded bg-danger-subtle px-2 py-1 text-xs text-danger-fg-soft">{error}</p>}

      {settings && (
        <>
          <label className="mb-2 flex items-center gap-2 text-sm text-fg-muted">
            <input
              type="checkbox"
              checked={settings.enabled}
              disabled={busy}
              onChange={(e) => handleToggleEnabled(e.target.checked)}
            />
            {t("mcpServer.enable")}
          </label>

          <div className="mb-3 space-y-2 rounded border border-border bg-surface p-3">
            <div>
              <p className="text-xs text-fg-subtle">{t("mcpServer.endpointLabel")}</p>
              <code className="block text-xs break-all text-fg-soft select-all">
                {settings.endpoint}
              </code>
            </div>
            <div>
              <p className="text-xs text-fg-subtle">{t("mcpServer.tokenLabel")}</p>
              <code
                className={`block text-xs break-all text-fg-soft ${tokenRevealed ? "select-all" : "select-none"}`}
              >
                {tokenRevealed ? settings.token : MASKED_TOKEN}
              </code>
            </div>
            <div className="flex items-center gap-2">
              <Button size="sm" onClick={() => setTokenRevealed((v) => !v)}>
                {tokenRevealed ? t("mcpServer.tokenHide") : t("mcpServer.tokenReveal")}
              </Button>
              <Button size="sm" onClick={handleRegenerateToken} disabled={busy}>
                {t("mcpServer.regenerateToken")}
              </Button>
            </div>
          </div>

          <div className="mb-3">
            <label className="mb-1 flex items-center gap-2 text-sm text-fg-soft">
              {t("mcpServer.confirmTimeoutLabel")}
              <input
                type="number"
                min={1}
                step={1}
                defaultValue={Math.round(settings.confirmTimeoutSecs / 60)}
                key={settings.confirmTimeoutSecs}
                disabled={busy}
                onBlur={(e) => handleChangeConfirmTimeoutMinutes(e.target.valueAsNumber)}
                className={`${FIELD_CONTROL_CLASS} w-16`}
              />
              {t("mcpServer.confirmTimeoutUnit")}
            </label>
            <p className="text-xs text-fg-subtle">{t("mcpServer.confirmTimeoutHint")}</p>
          </div>

          <div className="mb-3">
            <p className="mb-1 text-sm font-medium text-fg-soft">
              {t("mcpServer.allowListLabel")}
            </p>
            {servers.length === 0 ? (
              <p className="text-xs text-fg-subtle">{t("mcpServer.noServers")}</p>
            ) : (
              <ul className="max-h-40 divide-y divide-border overflow-y-auto rounded border border-border">
                {servers.map((server) => (
                  <li key={server.id} className="px-2 py-1.5">
                    <label className="flex items-center gap-2 text-sm text-fg-muted">
                      <input
                        type="checkbox"
                        checked={settings.allowedServerIds.includes(server.id)}
                        disabled={busy}
                        onChange={(e) => handleToggleAllowedServer(server.id, e.target.checked)}
                      />
                      <span className="min-w-0 truncate" title={server.name}>
                        {server.name}
                      </span>
                    </label>
                  </li>
                ))}
              </ul>
            )}
          </div>

          <p className="mb-1 text-xs text-fg-subtle">{t("mcpServer.exampleConfigHint")}</p>
          <pre
            className={`rounded border border-border bg-surface-sunken p-2 text-xs break-all whitespace-pre-wrap text-fg-muted ${tokenRevealed ? "" : "select-none"}`}
          >
            {exampleConfig(settings, tokenRevealed ? settings.token : MASKED_TOKEN)}
          </pre>
          <div className="mt-1 flex items-center gap-2">
            <Button size="sm" onClick={() => void handleCopyConfig()}>
              {t("mcpServer.copyConfig")}
            </Button>
            <span className="text-xs text-fg-subtle">{t("mcpServer.copyConfigNote")}</span>
            <output
              className={`text-xs ${copyStatus === "failed" ? "text-danger-fg-soft" : "text-fg-subtle"}`}
            >
              {copyStatus === "copied" && t("mcpServer.copyConfigCopied")}
              {copyStatus === "failed" && t("mcpServer.copyConfigFailed")}
            </output>
          </div>
        </>
      )}
    </div>
  );
}
