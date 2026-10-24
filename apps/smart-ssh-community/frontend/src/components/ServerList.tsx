import { useCallback, useEffect, useState } from "react";
import { NewSessionMenu } from "./NewSessionMenu";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { useTranslation } from "react-i18next";
import {
  commandErrorCode,
  commandErrorMessage,
  listChatSessions,
  listGroups,
  listServers,
  listUnusableServers,
  inspectKeyFile,
} from "../api";
import { translateErrorCode } from "../errorCodes";
import { loadFirstRunNoticeAcknowledged, saveFirstRunNoticeAcknowledged } from "../firstRunNotice";
import { buildGroupTree, type GroupTreeNode } from "../groupTree";
import {
  classifyDrop,
  dropHighlightClass,
  groupDropTargetValue,
  performMove,
  type DragItem,
  type DropTarget,
} from "../treeDrag";
import type {
  ChatSessionSummaryDto,
  ConnectStepRecord,
  GroupDto,
  ServerDto,
  UnusableServerDto,
} from "../types";
import { useTreeDrag } from "../useTreeDrag";
import { ChatSessionPickerScreen } from "./ChatSessionPickerScreen";
import { ConnectStepLog } from "./ConnectStepLog";
import { MoveToDialog } from "./MoveToDialog";
import { FirstRunNoticeScreen } from "./FirstRunNoticeScreen";
import { TreeDragGhost } from "./TreeDragGhost";
import { unusableReasonText } from "../unusableServer";
import { Button } from "./ui";

interface ServerListProps {
  /** Issue #548: opens a "Connecting to …" tab and runs the attempt there.
   * Issue #568: with `resumeChatSessionId` the attempt resumes that saved
   * chat session (also on Retry). */
  onStartConnect: (server: { id: string; name: string }, resumeChatSessionId?: string) => void;
  /** Issue #548: servers with a running connect attempt (keeps the
   * "Connecting…" state of the row). */
  connectingServerIds: ReadonlySet<string>;
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
  /** Issue #611: an extension's request to resume a saved chat session; it
   * takes the same path as choosing the session in the picker. */
  resumeRequest?: { serverId: string; chatSessionId: string } | null;
  onResumeRequestConsumed?: () => void;
  /** Issue #692: "Connect" in the server's settings — handled like a click
   * on the server row. */
  connectRequest?: { serverId: string } | null;
  onConnectRequestConsumed?: () => void;
  /** Spec 0069, Teil C1 (BL-0082): Klick auf "Ersten Server anlegen" im
   * Einstiegs-Block — wechselt zum Tab "Verwalten" mit geöffnetem
   * Neu-Formular (`App.tsx`s `handleCreateFirstServer`). */
  onCreateFirstServer: () => void;
  /** Issue #49: „+" an einer Gruppenzeile — wechselt zu „Verwalten" mit
   * geöffnetem Neu-Formular, diese Gruppe vorbelegt. */
  onCreateServerInGroup: (groupId: string) => void;
}

/** Spec 0047, Fund D2: bislang zeigte dieser Screen bei jedem Fehler nur
 * `commandErrorMessage(err)` — den rohen, immer deutschen `Display`-Text
 * des Backend-Fehlers, auch wenn ein `code` vorhanden und übersetzt wäre
 * (z. B. `SshError` beim eigentlichen Verbindungsaufbau, s.
 * `commands.rs::connect_session`). Nutzt stattdessen den bestehenden
 * Spec-0024-Mechanismus und fällt nur auf den rohen Text zurück, wenn
 * wirklich kein bekannter Code vorliegt. Als Modul-Funktion (statt einer
 * im Komponentenkörper geschlossenen Closure über `t`) definiert, damit
 * sie referenziell stabil ist und keine `useEffect`-Dependency wird. */
/** Issue #667 / Spec 0109 C3: names the key file as missing on this device
 * (e.g. after a backup import). A failed pre-check shows nothing — the file
 * is checked again when connecting. */
function KeyFileMissingNote({ path }: { path: string }) {
  const { t } = useTranslation();
  const [missing, setMissing] = useState(false);
  useEffect(() => {
    let cancelled = false;
    setMissing(false);
    inspectKeyFile(path)
      .then((facts) => {
        if (!cancelled) setMissing(facts.problem?.code === "KEY_FILE_NOT_FOUND");
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [path]);
  if (!missing) return null;
  return (
    <p className="text-xs text-amber-400">
      {t("errors.KEY_FILE_NOT_FOUND_ON_DEVICE")}
    </p>
  );
}

function describeError(
  t: (key: string, options?: Record<string, unknown>) => string,
  err: unknown,
): string {
  return translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err));
}

/**
 * Spec 0007, Abschnitt 7 / Spec 0033: Klick auf einen Server löst
 * `connect()` aus. Server erscheinen jetzt nach Gruppen-Hierarchie
 * gegliedert (Spec 0033, Abschnitt 3) statt als flache Liste — der lokale
 * Pseudo-Server (Spec 0032) fix angeheftet oberhalb aller Gruppen.
 * Die Host-Key-Abfrage (`host-key-verification-needed`) zeigt nicht diese
 * Liste, sondern das stets gemountete `HostKeyPromptHost` an der
 * `App`-Wurzel (Issue #12 / ADR 0104) — diese Liste ist bei aktivem
 * "Verwalten"-/"Filter-Regeln"-Tab nicht gemountet. Endet ein `connect()`
 * hier, schließt `clearHostKeyPrompt()` eine evtl. noch offene Abfrage.
 */
export function ServerList({
  onStartConnect,
  connectingServerIds,
  findExistingSessionId,
  onSwitchToExistingTab,
  collapsedGroupIds,
  onToggleGroup,
  resumeRequest = null,
  onResumeRequestConsumed,
  connectRequest = null,
  onConnectRequestConsumed,
  onCreateFirstServer,
  onCreateServerInGroup,
}: ServerListProps) {
  const { t } = useTranslation();
  const narrow = useIsNarrow();
  // Issue #582: row whose "Move to…" dialog is open (narrow layout).
  const [moveItem, setMoveItem] = useState<DragItem | null>(null);
  const [servers, setServers] = useState<ServerDto[]>([]);
  const [groups, setGroups] = useState<GroupDto[]>([]);
  // Issue #100: Server mit unlesbarer Anmeldeart — sichtbar, aber nicht
  // verbindbar.
  const [unusableServers, setUnusableServers] = useState<UnusableServerDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [connectFailure, setConnectFailure] = useState<{
    message: string;
    steps: ConnectStepRecord[];
  } | null>(null);
  // Spec 0031, Abschnitt 4: `null` = noch nicht geladen (Laden ist ein
  // schneller lokaler Store-Zugriff, ein kurzes Zeitfenster ohne Sperre
  // wird bewusst hingenommen — die eigentliche Durchsetzung sitzt ohnehin
  // serverseitig in `connect_session`, s. dortiger Kommentar; diese
  // Frontend-Sperre ist nur für eine bewusste, unmittelbare Rückmeldung
  // statt eines rohen Backend-Fehlers).
  const [firstRunAcknowledged, setFirstRunAcknowledged] = useState<boolean | null>(null);
  const [pendingConnectServer, setPendingConnectServer] = useState<ServerDto | null>(null);
  // Issue #611: chat session to resume once the first-run notice is done.
  const [pendingResumeId, setPendingResumeId] = useState<string | null>(null);
  // Spec 0034, Abschnitt 6: `null` = kein Auswahl-Screen offen. Enthält
  // sowohl den Server (für Titel/erneutes Nachladen nach Umbenennen/
  // Löschen) als auch die aktuell geladene Sitzungsliste.
  const [sessionPickerState, setSessionPickerState] = useState<{
    server: ServerDto;
    sessions: ChatSessionSummaryDto[];
  } | null>(null);

  const loadTree = useCallback(
    () =>
      Promise.all([listServers(), listGroups(), listUnusableServers()])
        .then(([s, g, u]) => {
          setServers(s);
          setGroups(g);
          setUnusableServers(u);
        })
        .catch((err) => setError(describeError(t, err)))
        .finally(() => setLoading(false)),
    [t],
  );

  useEffect(() => {
    void loadTree();
    // `t` aus `react-i18next` ist referenziell stabil (löst diesen Effekt
    // bei einem Sprachwechsel nicht erneut aus) — hier trotzdem korrekt
    // über `loadTree` gelistet, statt des sonst nötigen
    // exhaustive-deps-Lint-Suppressors.
  }, [loadTree]);

  /** Issue #48 / Spec 0103: Ablage eines gezogenen Servers/einer Gruppe.
   * `noop` (schon dort) ruft nichts auf; ein Zyklus geht bewusst ans
   * Backend, das ihn mit `GROUP_CYCLE_DETECTED` ablehnt — die Meldung
   * erscheint dann hier sichtbar, geändert wird nichts. */
  const handleDrop = async (item: DragItem, target: DropTarget) => {
    if (classifyDrop(item, target, groups, servers) === "noop") return;
    setError(null);
    try {
      await performMove(item, target);
    } catch (err) {
      setError(describeError(t, err));
    }
    await loadTree();
  };
  const { drag, handlersFor } = useTreeDrag((item, target) => void handleDrop(item, target), {
    touchDrag: !narrow,
  });
  const indent = (depth: number) => (narrow ? depth * 12 + 12 : depth * 16 + 16);
  const moreButton = (item: DragItem) => (
    <Button
      variant="ghost"
      onClick={() => setMoveItem(item)}
      title={t("treeDrag.moveTo", { name: item.label })}
      aria-label={t("treeDrag.moveTo", { name: item.label })}
      className="mr-1 min-h-11 min-w-11 shrink-0"
    >
      ⋯
    </Button>
  );
  const dropKind = drag?.target ? classifyDrop(drag.item, drag.target, groups, servers) : null;
  const highlightFor = (target: DropTarget) =>
    drag?.target &&
    drag.target.kind === target.kind &&
    (target.kind === "root" || (drag.target.kind === "group" && drag.target.id === target.id))
      ? dropHighlightClass(dropKind)
      : "";

  useEffect(() => {
    // Unabhängiger Review-Pass (Spec 0031): ohne `.catch` bleibt
    // `firstRunAcknowledged` bei einem Store-Ladefehler (z. B. defekte
    // `settings.json`) dauerhaft `null` — der Screen erscheint dann NIE
    // (nur bei explizit `false` wird er gezeigt, s. `handleConnect` unten),
    // jeder Verbindungsversuch läuft ins Backend-Gate und scheitert mit
    // einem rohen Fehler, ohne Weg zur Bestätigung. `false` ist hier der
    // sichere Fallback: zeigt den Hinweis, statt den Nutzer in eine
    // Sackgasse laufen zu lassen.
    loadFirstRunNoticeAcknowledged()
      .then(setFirstRunAcknowledged)
      .catch(() => setFirstRunAcknowledged(false));
  }, []);

  const [newSessionMenu, setNewSessionMenu] = useState<{
    x: number;
    y: number;
    server: ServerDto;
  } | null>(null);
  const closeNewSessionMenu = useCallback(() => setNewSessionMenu(null), []);

  const performConnect = async (server: ServerDto, resumeChatSessionId?: string) => {
    setError(null);
    setConnectFailure(null);
    const target = { id: server.id, name: server.name };
    if (resumeChatSessionId) onStartConnect(target, resumeChatSessionId);
    else onStartConnect(target);
  };

  // Issue #611: same checks as a click on the server plus the picker's
  // resume choice: open tab → switch; first-run notice → show it first;
  // otherwise the connecting tab resumes the chat session.
  useEffect(() => {
    if (!resumeRequest || loading || firstRunAcknowledged === null) return;
    onResumeRequestConsumed?.();
    const existing = findExistingSessionId(resumeRequest.serverId);
    if (existing) {
      onSwitchToExistingTab(existing);
      return;
    }
    const server = servers.find((s) => s.id === resumeRequest.serverId);
    if (!server) {
      console.error("resume request for an unknown or unusable server");
      return;
    }
    if (firstRunAcknowledged === false) {
      setPendingResumeId(resumeRequest.chatSessionId);
      setPendingConnectServer(server);
      return;
    }
    void performConnect(server, resumeRequest.chatSessionId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [resumeRequest, loading, firstRunAcknowledged, servers]);

  // Issue #692: same flow as a click on the server (open tab → switch,
  // first-run notice, session picker, connecting tab).
  useEffect(() => {
    if (!connectRequest || loading || firstRunAcknowledged === null) return;
    onConnectRequestConsumed?.();
    const server = servers.find((s) => s.id === connectRequest.serverId);
    if (!server) {
      console.error("connect request for an unknown or unusable server");
      return;
    }
    void handleConnect(server);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connectRequest, loading, firstRunAcknowledged, servers]);

  /** Spec 0031, Abschnitt 4: der Screen blockiert nur den ersten
   * `connect()`-Aufruf, nicht die übrige App-Nutzung davor — deshalb hier
   * am eigentlichen Verbindungsaufbau abgefangen statt z. B. den ganzen
   * Screen beim App-Start zu sperren.
   *
   * Spec 0034, Abschnitt 6: "Hat ein Server noch keine gespeicherte
   * Sitzung: kein Auswahl-Screen, direktes Verbinden wie bisher" — der
   * lokale Pseudo-Server hat nie eine gespeicherte Sitzung (Spec 0032:
   * keine `servers`-Zeile, s. `commands::connect_session`s
   * `is_local`-Zweig), `listChatSessions` liefert für ihn also ohnehin
   * immer eine leere Liste; kein gesonderter Fall hier nötig.
   */
  const handleConnect = async (server: ServerDto) => {
    // Issue #548: also covers a running connect attempt (its tab is
    // focused instead of starting a second attempt).
    const existingSessionId = findExistingSessionId(server.id);
    if (existingSessionId) {
      onSwitchToExistingTab(existingSessionId);
      return;
    }
    // Issue #407: the buttons stay focusable (aria-disabled instead of
    // disabled) while an attempt is pending, so the guard lives here.
    if (firstRunAcknowledged === false) {
      setPendingConnectServer(server);
      return;
    }
    await openSessionPickerOrConnect(server);
  };

  // Issue #657: "New session" always opens an additional tab, even if the
  // server already has one (no session picker: it is a fresh session).
  const handleNewSession = async (server: ServerDto) => {
    if (firstRunAcknowledged === false) {
      setPendingConnectServer(server);
      return;
    }
    await performConnect(server);
  };

  const openSessionPickerOrConnect = async (server: ServerDto) => {
    setError(null);
    try {
      const sessions = await listChatSessions(server.id);
      if (sessions.length === 0) {
        await performConnect(server);
        return;
      }
      setSessionPickerState({ server, sessions });
    } catch (err) {
      setError(describeError(t, err));
    }
  };

  const handleResumeSession = (sessionId: string) => {
    const server = sessionPickerState?.server;
    if (!server) return;
    setSessionPickerState(null);
    setError(null);
    setConnectFailure(null);
    onStartConnect({ id: server.id, name: server.name }, sessionId);
  };

  const reloadSessionPicker = async () => {
    const server = sessionPickerState?.server;
    if (!server) return;
    try {
      const sessions = await listChatSessions(server.id);
      setSessionPickerState({ server, sessions });
    } catch (err) {
      setError(describeError(t, err));
    }
  };

  const handleFirstRunNoticeAcknowledged = async (afterStored: () => void) => {
    // Unabhängiger Review-Pass (Spec 0031): `setFirstRunAcknowledged(true)`
    // lief vorher VOR dem `await` unten — schlug das Speichern fehl, glaubte
    // das Frontend fortan "bestätigt", während der Store weiter `false`
    // sagt. Der Screen wäre dann bis zum Neustart nie wieder erschienen,
    // jeder weitere Verbindungsversuch wäre am Backend-Gate gescheitert.
    // Jetzt wird der lokale Zustand erst NACH erfolgreichem Speichern
    // gesetzt.
    const server = pendingConnectServer;
    const resumeId = pendingResumeId;
    setPendingConnectServer(null);
    setPendingResumeId(null);
    try {
      await saveFirstRunNoticeAcknowledged();
    } catch (err) {
      setError(describeError(t, err));
      return;
    }
    setFirstRunAcknowledged(true);
    // Spec 0031, Abschnitt 6: Erweiterungs-Handler erst nach erfolgreichem
    // Speichern; sie blockieren den Verbindungsaufbau nicht.
    afterStored();
    if (server) {
      await performConnect(server, resumeId ?? undefined);
    }
  };

  const renderServerRow = (server: ServerDto, depth: number) => (
    <li key={server.id} className={narrow ? "flex items-center" : undefined}>
      <button
        type="button"
        // Issue #48: der lokale Pseudo-Server (Spec 0032) ist nicht ziehbar.
        {...(server.isLocal
          ? {}
          : handlersFor({ kind: "server", id: server.id, label: server.name }))}
        onClick={() => handleConnect(server)}
        onContextMenu={(e) => {
          e.preventDefault();
          setNewSessionMenu({ x: e.clientX, y: e.clientY, server });
        }}
       
        style={{ paddingLeft: `${indent(depth)}px` }}
        className="flex w-full min-w-0 select-none items-center justify-between py-3 pr-4 text-left narrow:min-h-11 narrow:flex-1 narrow:flex-wrap narrow:gap-y-1 narrow:pr-2 hover:bg-slate-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo-400 aria-disabled:opacity-60"
      >
        <div className="min-w-0">
          <p className="font-medium break-words text-slate-100">{server.name}</p>
          {/* Spec 0046, Fund 5: der lokale Pseudo-Server hat konzeptionell
           * keinen Port (direkte Prozessausführung, kein SSH/TCP) —
           * `server.port` wäre hier immer `0` (s. `local_server::
           * synthetic_server`), das würde einen nicht existierenden Port
           * suggerieren statt ihn einfach auszublenden. */}
          <p className="text-sm text-slate-400 narrow:break-all">
            {server.isLocal
              ? `${server.username}@${server.host}`
              : `${server.username}@${server.host}:${server.port}`}
          </p>
          {/* Spec 0076, B-4: erkennbar, dass die Anmeldung an einer Datei
           * hängt, und wo sie liegt — `title` zeigt den vollen Pfad, falls
           * er abgeschnitten wird. */}
          {server.identityFilePath && (
            <p className="truncate text-xs text-slate-400" title={server.identityFilePath}>
              {/* spec-reviewer-Fund (Review dieses Schritts): das Emoji
               * allein trägt keine barrierefreie Bedeutung — `aria-hidden`
               * nimmt es aus dem Accessibility-Baum, der sichtbare Pfad
               * daneben bleibt der eigentliche Inhalt der Zeile. */}
              <span aria-hidden="true">🔑</span> {server.identityFilePath}
            </p>
          )}
          {server.identityFilePath && <KeyFileMissingNote path={server.identityFilePath} />}
        </div>
        <div className="flex items-center gap-2 narrow:flex-wrap">
          {server.tags.map((tag) => (
            <span key={tag} className="rounded bg-slate-700 px-2 py-0.5 text-xs text-slate-200">
              {tag}
            </span>
          ))}
          {connectingServerIds.has(server.id) && (
            <span className="text-xs text-indigo-300">{t("mainScreen.connecting")}</span>
          )}
        </div>
      </button>
      {narrow && !server.isLocal && moreButton({ kind: "server", id: server.id, label: server.name })}
    </li>
  );

  /** Issue #100: ein nicht nutzbarer Server — bewusst kein Button: kein
   * Verbinden, kein Ziehen. Begründung sichtbar, Löschen unter „Verwalten". */
  const renderUnusableServerRow = (server: UnusableServerDto, depth: number) => (
    <li key={server.id} data-testid="unusable-server-row">
      <div
        style={{ paddingLeft: `${indent(depth)}px` }}
        className="flex w-full select-none items-center justify-between gap-2 py-3 pr-4 text-left opacity-70 narrow:flex-wrap narrow:pr-2"
      >
        <div className="min-w-0 break-words">
          <p className="font-medium text-slate-300">
            <span aria-hidden="true">⚠️</span> {server.name}
          </p>
          <p className="text-sm text-slate-400">{server.host}</p>
          <p className="text-xs text-amber-300">{unusableReasonText(t, server.reason)}</p>
          <p className="text-xs text-slate-400">
            {t("unusableServer.listHint", { manage: t("nav.manage") })}
          </p>
        </div>
        <span className="rounded bg-amber-900 px-2 py-0.5 text-xs text-amber-200">
          {t("unusableServer.badge")}
        </span>
      </div>
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
      <li
        key={node.group.id}
        data-drop-target={groupDropTargetValue(node.group.id)}
        className={`border-b border-slate-700 last:border-b-0 ${highlightFor({ kind: "group", id: node.group.id })}`}
      >
        {/* Issue #49: Auf-/Zuklappen und „+" (neuer Server in dieser
         * Gruppe) als Geschwister — ein Button im Button wäre ungültig. */}
        <div className="flex items-center hover:bg-slate-800">
          <button
            type="button"
            {...handlersFor({ kind: "group", id: node.group.id, label: node.group.name })}
            onClick={() => onToggleGroup(node.group.id)}
            title={node.group.name}
            style={{ paddingLeft: `${indent(depth)}px` }}
            className="flex min-w-0 flex-1 select-none items-center gap-2 py-2 narrow:min-h-11 pr-2 text-left text-sm font-medium text-slate-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo-400"
          >
            <span className="w-3 text-slate-400">{collapsed ? "▸" : "▾"}</span>
            <span className="truncate" title={node.group.name}>
              📁 {node.group.name}
            </span>
          </button>
          <Button variant="ghost" size="sm"
            onClick={() => onCreateServerInGroup(node.group.id)}
            title={t("mainScreen.newServerInGroup", { name: node.group.name })}
            aria-label={t("mainScreen.newServerInGroup", { name: node.group.name })}
            className="mr-3 narrow:mr-1 narrow:min-h-11 narrow:min-w-11"
          >
            +
          </Button>
          {narrow && moreButton({ kind: "group", id: node.group.id, label: node.group.name })}
        </div>
        {!collapsed &&
          (node.children.length > 0 ||
            node.servers.length > 0 ||
            node.unusableServers.length > 0) && (
          <ul className="divide-y divide-slate-800">
            {node.children.map((child) => renderGroupSection(child, depth + 1))}
            {node.servers.map((s) => renderServerRow(s, depth + 1))}
            {node.unusableServers.map((s) => renderUnusableServerRow(s, depth + 1))}
          </ul>
        )}
      </li>
    );
  };

  if (loading) {
    return <p className="text-sm text-slate-400">{t("mainScreen.loadingServers")}</p>;
  }

  const localServer = servers.find((s) => s.isLocal);
  const tree = buildGroupTree(groups, servers, unusableServers);
  // Spec 0069, Teil C1 (BL-0082): "leer" für den Einstiegs-Block heißt
  // ausschließlich "keine echten Server" — der lokale Pseudo-Server zählt
  // nie mit, Gruppen (auch leere) zählen für DIESE Bedingung nicht: eine
  // Gruppe ohne Server ist kein Einstieg in den Pfad. Bewusst getrennt von
  // der Frage unten, ob der Gruppenbaum überhaupt etwas zu zeigen hat
  // (die zwei Bedingungen sind nicht dasselbe — nur-Gruppen-ohne-Server
  // zeigt BEIDES: Block und Baum).
  // Issue #100: ein nicht nutzbarer Server zählt mit — er existiert, der
  // Einstiegs-Block „ersten Server anlegen" wäre irreführend.
  const hasRealServers = servers.some((s) => !s.isLocal) || unusableServers.length > 0;
  // Spec 0032, Abschnitt 3: der lokale Pseudo-Server ist immer vorhanden,
  // `servers` ist deshalb nie tatsächlich leer.
  const hasGroupTreeContent =
    groups.length > 0 ||
    tree.ungroupedServers.length > 0 ||
    tree.ungroupedUnusableServers.length > 0;

  return (
    <>
      {error && <p className="mb-2 text-sm text-red-400">{error}</p>}
      {error && connectFailure && connectFailure.message === error && (
        <div className="mb-2">
          <ConnectStepLog steps={connectFailure.steps} />
        </div>
      )}

      {/* Spec 0032, Abschnitt 5 / Spec 0033, Abschnitt 3: fix oberhalb aller
       * Gruppen-Bereiche, visuell durch die eigene Box + den Abstand zur
       * Gruppen-Liste klar abgesetzt — nie innerhalb eines Ordners. */}
      {localServer && (
        <ul className="mb-4 divide-y divide-slate-700 rounded-md border border-slate-700">
          {renderServerRow(localServer, 0)}
        </ul>
      )}

      {/* Spec 0069, Teil C1 (BL-0082): ersetzt den früheren
       * Entwicklertext ("s. profiles_demo-Beispiel..."). Existieren
       * Gruppen, wird der Gruppenbaum unten ZUSÄTZLICH gezeigt (nicht
       * ausschließend) — s. `hasGroupTreeContent`. */}
      {!hasRealServers && !error && (
        <div className="mb-4 rounded-md border border-slate-700 bg-slate-900/40 p-4">
          <p className="text-sm font-medium text-slate-100">
            {t("mainScreen.emptyStateTitle")}
          </p>
          <p className="mt-1 text-sm text-slate-400">{t("mainScreen.emptyStateHint")}</p>
          <Button variant="primary"
            onClick={onCreateFirstServer}
            className="mt-3"
          >
            {t("mainScreen.emptyStateCreateFirstServer")}
          </Button>
          <p className="mt-3 text-xs text-slate-400">{t("mainScreen.emptyStateTryLocalhost")}</p>
        </div>
      )}

      {hasGroupTreeContent && (
        <ul className="divide-y divide-slate-700 rounded-md border border-slate-700">
          {tree.roots.map((node) => renderGroupSection(node, 0))}
          {/* Issue #48: "Ohne Gruppe" ist zugleich das Ablageziel für die
           * Wurzelebene — beim Ziehen auch dann sichtbar, wenn noch kein
           * Server ungruppiert ist. */}
          {(tree.ungroupedServers.length > 0 ||
            tree.ungroupedUnusableServers.length > 0 ||
            drag) && (
            <li
              data-drop-target="root"
              className={`border-b border-slate-700 last:border-b-0 ${highlightFor({ kind: "root" })}`}
            >
              <div className="px-4 py-2 text-sm font-medium text-slate-200">
                {t("mainScreen.ungrouped")}
              </div>
              {tree.ungroupedServers.length > 0 || tree.ungroupedUnusableServers.length > 0 ? (
                <ul className="divide-y divide-slate-800">
                  {tree.ungroupedServers.map((s) => renderServerRow(s, 1))}
                  {tree.ungroupedUnusableServers.map((s) => renderUnusableServerRow(s, 1))}
                </ul>
              ) : (
                <p className="mx-4 mb-2 rounded border border-dashed border-slate-600 px-3 py-2 text-xs text-slate-400">
                  {t("treeDrag.rootDropZone")}
                </p>
              )}
            </li>
          )}
        </ul>
      )}

      {moveItem && (
        <MoveToDialog
          item={moveItem}
          groups={groups}
          servers={servers}
          onMove={(item, target) => void handleDrop(item, target)}
          onClose={() => setMoveItem(null)}
        />
      )}

      {drag && <TreeDragGhost drag={drag} kind={dropKind} />}

      {newSessionMenu && (
        <NewSessionMenu
          x={newSessionMenu.x}
          y={newSessionMenu.y}
          serverName={newSessionMenu.server.name}
          onClose={closeNewSessionMenu}
          onNewSession={() => {
            const server = newSessionMenu.server;
            setNewSessionMenu(null);
            void handleNewSession(server);
          }}
        />
      )}

      {pendingConnectServer && (
        <FirstRunNoticeScreen onAcknowledge={handleFirstRunNoticeAcknowledged} />
      )}
      {sessionPickerState && (
        <ChatSessionPickerScreen
          serverName={sessionPickerState.server.name}
          sessions={sessionPickerState.sessions}
          onNewConversation={() => {
            const server = sessionPickerState.server;
            setSessionPickerState(null);
            performConnect(server);
          }}
          onResume={handleResumeSession}
          onSessionsChanged={reloadSessionPicker}
          onCancel={() => setSessionPickerState(null)}
        />
      )}
    </>
  );
}
