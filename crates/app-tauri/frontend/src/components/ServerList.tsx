import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, confirmHostKey, connect, listGroups, listServers } from "../api";
import { onHostKeyVerificationNeeded } from "../events";
import { loadFirstRunNoticeAcknowledged, saveFirstRunNoticeAcknowledged } from "../firstRunNotice";
import { buildGroupTree, type GroupTreeNode } from "../groupTree";
import type { GroupDto, HostKeyVerificationNeededEvent, ServerDto } from "../types";
import { FirstRunNoticeScreen } from "./FirstRunNoticeScreen";
import { HostKeyDialog } from "./HostKeyDialog";

interface ServerListProps {
  onConnected: (sessionId: string, serverName: string, serverId: string) => void;
  /** Spec 0017, Abschnitt 3: "existiert bereits ein Tab für diesen Server,
   * wird stattdessen zu diesem gewechselt (kein zweiter Tab zum selben
   * Server)" — die Prüfung muss **vor** `connect()` passieren, sonst würde
   * für einen bereits offenen Server unnötig eine zweite SSH-Verbindung
   * aufgebaut und sofort wieder verworfen. */
  findExistingSessionId: (serverId: string) => string | undefined;
  onSwitchToExistingTab: (sessionId: string) => void;
  /** Spec 0033, Abschnitt 4: Auf-/Zuklapp-Zustand pro Gruppe — vom
   * Aufrufer (`App.tsx`, überlebt einen Tab-Wechsel weg von "Verbinden")
   * gehalten, damit er "mindestens für die laufende Sitzung" erhalten
   * bleibt, statt bei jedem Remount dieser Komponente zurückzufallen. */
  collapsedGroupIds: Set<string>;
  onToggleGroup: (groupId: string) => void;
}

/**
 * Spec 0007, Abschnitt 7 / Spec 0033: Klick auf einen Server löst
 * `connect()` aus. Server erscheinen jetzt nach Gruppen-Hierarchie
 * gegliedert (Spec 0033, Abschnitt 3) statt als flache Liste — der lokale
 * Pseudo-Server (Spec 0032) fix angeheftet oberhalb aller Gruppen.
 * Reagiert auf `host-key-verification-needed` mit `HostKeyDialog` — der
 * Event-Listener läuft, solange irgendein `connect()` dieser Liste
 * unterwegs ist (nicht dauerhaft), da die `session_id` im Event erst durch
 * den laufenden `connect()`-Aufruf entsteht (s. Backend-Kommentar zu
 * `commands::connect`).
 */
export function ServerList({
  onConnected,
  findExistingSessionId,
  onSwitchToExistingTab,
  collapsedGroupIds,
  onToggleGroup,
}: ServerListProps) {
  const { t } = useTranslation();
  const [servers, setServers] = useState<ServerDto[]>([]);
  const [groups, setGroups] = useState<GroupDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [pendingHostKey, setPendingHostKey] = useState<HostKeyVerificationNeededEvent | null>(
    null,
  );
  // Spec 0031, Abschnitt 4: `null` = noch nicht geladen (Laden ist ein
  // schneller lokaler Store-Zugriff, ein kurzes Zeitfenster ohne Sperre
  // wird bewusst hingenommen — die eigentliche Durchsetzung sitzt ohnehin
  // serverseitig in `connect_session`, s. dortiger Kommentar; diese
  // Frontend-Sperre ist nur für eine bewusste, unmittelbare Rückmeldung
  // statt eines rohen Backend-Fehlers).
  const [firstRunAcknowledged, setFirstRunAcknowledged] = useState<boolean | null>(null);
  const [pendingConnectServer, setPendingConnectServer] = useState<ServerDto | null>(null);

  useEffect(() => {
    Promise.all([listServers(), listGroups()])
      .then(([s, g]) => {
        setServers(s);
        setGroups(g);
      })
      .catch((err) => setError(commandErrorMessage(err)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    loadFirstRunNoticeAcknowledged().then(setFirstRunAcknowledged);
  }, []);

  useEffect(() => {
    const unlisten = onHostKeyVerificationNeeded((event) => setPendingHostKey(event));
    return () => {
      unlisten.then((unlistenFn) => unlistenFn());
    };
  }, []);

  const performConnect = async (server: ServerDto) => {
    setError(null);
    setConnectingId(server.id);
    try {
      const sessionId = await connect(server.id);
      onConnected(sessionId, server.name, server.id);
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setConnectingId(null);
      setPendingHostKey(null);
    }
  };

  /** Spec 0031, Abschnitt 4: der Screen blockiert nur den ersten
   * `connect()`-Aufruf, nicht die übrige App-Nutzung davor — deshalb hier
   * am eigentlichen Verbindungsaufbau abgefangen statt z. B. den ganzen
   * Screen beim App-Start zu sperren. */
  const handleConnect = async (server: ServerDto) => {
    const existingSessionId = findExistingSessionId(server.id);
    if (existingSessionId) {
      onSwitchToExistingTab(existingSessionId);
      return;
    }
    if (firstRunAcknowledged === false) {
      setPendingConnectServer(server);
      return;
    }
    await performConnect(server);
  };

  const handleFirstRunNoticeAcknowledged = async () => {
    setFirstRunAcknowledged(true);
    const server = pendingConnectServer;
    setPendingConnectServer(null);
    try {
      await saveFirstRunNoticeAcknowledged();
    } catch (err) {
      setError(commandErrorMessage(err));
      return;
    }
    if (server) await performConnect(server);
  };

  const handleHostKeyDecision = async (decision: Parameters<typeof confirmHostKey>[1]) => {
    if (!pendingHostKey) return;
    const sessionId = pendingHostKey.sessionId;
    setPendingHostKey(null);
    try {
      await confirmHostKey(sessionId, decision);
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  const renderServerRow = (server: ServerDto, depth: number) => (
    <li key={server.id}>
      <button
        type="button"
        onClick={() => handleConnect(server)}
        disabled={connectingId !== null}
        style={{ paddingLeft: `${depth * 16 + 16}px` }}
        className="flex w-full items-center justify-between py-3 pr-4 text-left hover:bg-slate-800 disabled:opacity-60"
      >
        <div>
          <p className="font-medium text-slate-100">{server.name}</p>
          <p className="text-sm text-slate-400">
            {server.username}@{server.host}:{server.port}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {server.tags.map((tag) => (
            <span key={tag} className="rounded bg-slate-700 px-2 py-0.5 text-xs text-slate-200">
              {tag}
            </span>
          ))}
          {connectingId === server.id && <span className="text-xs text-indigo-300">Verbinde…</span>}
        </div>
      </button>
    </li>
  );

  /** Spec 0033, Abschnitt 3/4: eine Gruppe erscheint als eigener,
   * auf-/zuklappbarer Abschnitt, Untergruppen darin rekursiv eingerückt.
   * Bewusst KEINE Filterung "nur wenn Server enthalten" (Abschnitt 4:
   * eine leere Gruppe mit Untergruppen, die selbst Server enthalten, muss
   * trotzdem erscheinen) — `buildGroupTree` liefert dafür bereits die
   * vollständige, ungefilterte Hierarchie. */
  const renderGroupSection = (node: GroupTreeNode, depth: number) => {
    const collapsed = collapsedGroupIds.has(node.group.id);
    return (
      <li key={node.group.id} className="border-b border-slate-700 last:border-b-0">
        <button
          type="button"
          onClick={() => onToggleGroup(node.group.id)}
          style={{ paddingLeft: `${depth * 16 + 16}px` }}
          className="flex w-full items-center gap-2 py-2 pr-4 text-left text-sm font-medium text-slate-200 hover:bg-slate-800"
        >
          <span className="w-3 text-slate-500">{collapsed ? "▸" : "▾"}</span>
          📁 {node.group.name}
        </button>
        {!collapsed && (node.children.length > 0 || node.servers.length > 0) && (
          <ul className="divide-y divide-slate-800">
            {node.children.map((child) => renderGroupSection(child, depth + 1))}
            {node.servers.map((s) => renderServerRow(s, depth + 1))}
          </ul>
        )}
      </li>
    );
  };

  if (loading) {
    return <p className="text-sm text-slate-400">Lade Server…</p>;
  }

  const localServer = servers.find((s) => s.isLocal);
  const tree = buildGroupTree(groups, servers);
  // Spec 0032, Abschnitt 3: der lokale Pseudo-Server ist immer vorhanden,
  // `servers` ist deshalb nie tatsächlich leer — "nichts angelegt" heißt
  // hier: außer ihm gibt es keine echten Server/Gruppen.
  const hasNoRealServersOrGroups = groups.length === 0 && tree.ungroupedServers.length === 0;

  return (
    <>
      {error && <p className="mb-2 text-sm text-red-400">{error}</p>}

      {/* Spec 0032, Abschnitt 5 / Spec 0033, Abschnitt 3: fix oberhalb aller
       * Gruppen-Bereiche, visuell durch die eigene Box + den Abstand zur
       * Gruppen-Liste klar abgesetzt — nie innerhalb eines Ordners. */}
      {localServer && (
        <ul className="mb-4 divide-y divide-slate-700 rounded-md border border-slate-700">
          {renderServerRow(localServer, 0)}
        </ul>
      )}

      {hasNoRealServersOrGroups && !error ? (
        <p className="text-sm text-slate-400">
          Noch keine weiteren Server angelegt (s. <code>profiles_demo</code>-Beispiel oder
          CLI-Helfer, solange es noch keine Anlege-UI gibt).
        </p>
      ) : (
        <ul className="divide-y divide-slate-700 rounded-md border border-slate-700">
          {tree.roots.map((node) => renderGroupSection(node, 0))}
          {tree.ungroupedServers.length > 0 && (
            <li className="border-b border-slate-700 last:border-b-0">
              <div className="px-4 py-2 text-sm font-medium text-slate-200">
                {t("mainScreen.ungrouped")}
              </div>
              <ul className="divide-y divide-slate-800">
                {tree.ungroupedServers.map((s) => renderServerRow(s, 1))}
              </ul>
            </li>
          )}
        </ul>
      )}

      {pendingHostKey && (
        <HostKeyDialog event={pendingHostKey} onDecision={handleHostKeyDecision} />
      )}
      {pendingConnectServer && (
        <FirstRunNoticeScreen onAcknowledge={handleFirstRunNoticeAcknowledged} />
      )}
    </>
  );
}
