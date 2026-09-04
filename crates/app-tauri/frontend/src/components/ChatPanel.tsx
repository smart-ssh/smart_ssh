import { type FormEvent, type KeyboardEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  acceptAndCreateRule,
  cancelRunningCommand,
  commandErrorMessage,
  exportDocument,
  getChatHistory,
  listAiProviders,
  listPromptHistory,
  respondToAction,
  sendChatMessage,
  stopAutoContinuation,
  suggestRulePatterns,
  takeChatContentIntoNote,
} from "../api";
import {
  onChatActionProposed,
  onChatActionResult,
  onChatAutoContinuationLimitReached,
  onChatAutoContinuationStarted,
  onChatDocumentGenerated,
  onChatError,
  onChatTextDelta,
  onRiskAssessmentUpdated,
} from "../events";
import { translateErrorCode } from "../errorCodes";
import { formatBytes } from "../format";
import {
  initialHistoryNavState,
  navigateHistory,
  type HistoryNavState,
} from "../promptHistoryNav";
import { loadRiskClassifierSettings } from "../riskSettings";
import type {
  ActionOrigin,
  ActionResultPayload,
  ActionUserDecision,
  AiAction,
  Decision,
  DocumentFormat,
  PatternSuggestionDto,
  PatternType,
  RiskAssessment,
  Scope,
} from "../types";
import { NoteDiffPreview } from "./NoteDiffPreview";

export type ChatItem =
  | { type: "user"; id: string; text: string }
  | { type: "assistant"; id: string; text: string }
  | {
      type: "action";
      id: string;
      actionId: string;
      action: AiAction;
      decision: Decision;
      /** Optimistisches lokales Flag — Deny über `respond_to_action` löst
       * (anders als Approve/EditThenApprove) kein `chat-action-result` aus,
       * die Buttons müssen also unabhängig davon verschwinden. */
      responded: boolean;
      /** Spec 0021, Abschnitt 6: welche Entscheidung der Nutzer im Dialog
       * getroffen hat (nur gesetzt, sobald `responded`) — unabhängig vom
       * ursprünglichen `decision`-Feld (das bleibt `Confirm{reason}`, auch
       * nachdem der Nutzer abgelehnt hat). Bestimmt, ob die Karte als
       * "abgelehnt" markiert wird, statt weiter "Bestätigung nötig" zu
       * zeigen. */
      userDecision?: "approve" | "deny" | "editThenApprove";
      result?: ActionResultPayload;
      /** Spec 0019, Abschnitt 3/4 — nur bei `ProposeNoteUpdate` gesetzt. */
      previousNoteContent: string | null;
      /** Spec 0018, Abschnitt 7 — nur bei `SuggestCommand` relevant. */
      usesStoredSudoPassword: boolean;
      /** Spec 0020, Abschnitt 4.2, Punkt 3 — nur bei `WriteRemoteFile`
       * gesetzt (`null` bei neuer oder bei binärer Datei — dann
       * unterscheidet `previousFileSize`). */
      previousFileContent: string | null;
      previousFileSize: number | null;
      /** Spec 0023, Abschnitt 3 — nur bei `ProposeNoteUpdate` gesetzt,
       * *immer* angezeigt (auch wenn es der aktuell offene Server der
       * Session ist — Konsistenz statt Redundanzvermeidung). */
      targetName: string | null;
      /** Spec 0026, Abschnitt 2 — `null` für `ProposeNoteUpdate` (deckt
       * Spec 0026 nicht ab). Das `dataRisk`/`dataRiskReason`-Paar wird ggf.
       * später über `risk-assessment-updated` angehoben (s.
       * `riskSecondOpinionPending`). */
      riskAssessment: RiskAssessment | null;
      /** Spec 0026, Abschnitt 4: Lade-Indikator neben dem Daten-Badge,
       * solange eine aktivierte KI-Zweitmeinung noch aussteht — `true` ab
       * dem ersten Event, sobald `riskAssessment` gesetzt UND `!aiReviewed`
       * ist (heißt: eine Zweitmeinung ist grundsätzlich möglich, ihr
       * Ergebnis aber noch nicht da), `false` sobald `risk-assessment-
       * updated` für diese `actionId` ankommt. Bleibt `false`, wenn die
       * Zweitmeinung deaktiviert ist (dann kommt gar kein Update-Event). */
      riskSecondOpinionPending: boolean;
      /** Spec 0027, Abschnitt 2 — Zeitpunkt (`Date.now()`), an dem die
       * tatsächliche Ausführung begann: sofort bei `AutoExec`, sonst erst
       * nach Klick auf "Ausführen"/"editierten Vorschlag ausführen" im
       * Bestätigungsdialog. `null`, solange noch nicht ausgeführt wird
       * (wartet auf Bestätigung) — rein clientseitig, kein Backend-Event
       * nötig (s. Spec). Nur für `SuggestCommand` überhaupt relevant. */
      startedAt: number | null;
      /** Spec 0028, Abschnitt 6/9a: `mcp` zeigt "Angefragt über: <Client
       * oder generischer Text>" statt des internen Chat-Flows. */
      origin: ActionOrigin;
    }
  | { type: "error"; id: string; message: string; code: string | null }
  | { type: "autoContinuationLimitReached"; id: string; limit: number }
  | { type: "document"; id: string; title: string; contentMarkdown: string }
  // Spec 0034, Abschnitt 6/8: read-only Darstellung eines Kommandoergebnis-
  // /Ablehnungs-Eintrags aus einer bereits geladenen (fortgesetzten)
  // Historie — anders als "action" kein Bestätigungsdialog/Live-Zustand,
  // nur Anzeige dessen, was bereits geschehen ist.
  | {
      type: "historyCommandResult";
      id: string;
      command: string;
      stdout: string;
      stderr: string;
      exitCode: number | null;
      cancelled: boolean;
    }
  | { type: "historyRejected"; id: string; command: string; reason: string };

/** Spec 0023, Abschnitt 3/4: `targetName` wird für `ProposeNoteUpdate`
 * *immer* im Label gezeigt — auch wenn es der aktuell offene Server der
 * Session ist (Konsistenz statt Redundanzvermeidung, verhindert genau die
 * gemeldete Bug-Klasse, falls sich der Anzeigekontext künftig ändert,
 * z. B. durch Multi-Tab). Gruppen-Ziele bekommen das eigene Wort
 * "Gruppen-Notiz" (nicht "Notiz (Server X)"), um die Verwechslungsgefahr
 * auf der Server-vs-Gruppe-Achse zu vermeiden. `targetName` kommt nur bei
 * `ProposeNoteUpdate` befüllt an (s. `ChatItem`-Doc-Kommentar) — für alle
 * anderen Aktionstypen bleibt der Parameter ungenutzt.
 */
function describeAction(
  t: (key: string, options?: Record<string, unknown>) => string,
  action: AiAction,
  targetName: string | null,
): { label: string; command?: string } {
  if ("SuggestCommand" in action) {
    return { label: t("actionCard.suggestCommand"), command: action.SuggestCommand.command };
  }
  if ("GenerateDocument" in action) {
    return { label: t("actionCard.generateDocument", { title: action.GenerateDocument.title }) };
  }
  if ("ReadRemoteFile" in action) {
    return { label: t("actionCard.readFile", { path: action.ReadRemoteFile.path }) };
  }
  if ("WriteRemoteFile" in action) {
    return { label: t("actionCard.writeFile", { path: action.WriteRemoteFile.path }) };
  }
  const name = targetName ?? t("actionCard.unknownTarget");
  const isGroup = action.ProposeNoteUpdate.target === "CurrentServerGroup";
  return {
    label: isGroup
      ? t("actionCard.updateNoteGroup", { name })
      : t("actionCard.updateNoteServer", { name }),
  };
}

/// `Decision::Confirm`/`Deny`-Reasons aus der Filter-Engine sind
/// `"; "`-getrennte Einzelgründe (`crate::filter::engine::merge_reasons`).
/// Als ein durchgehender Satz mit Semikola wirkt das bei mehreren Gründen
/// unübersichtlich — hier stattdessen als kompakte, durch " · " getrennte
/// Liste dargestellt.
function formatReason(reason: string): string {
  return reason
    .split("; ")
    .filter((part, index, all) => all.indexOf(part) === index)
    .join(" · ");
}

const MCP_CLIENT_NAME_MAX_LENGTH = 40;

/** Unabhängiger Review-Pass (Spec 0028): `clientName` ist ein vom
 * MCP-Client selbst gewählter, ungeprüfter String (Handshake-`clientInfo.
 * name`) — ohne Längenbegrenzung könnte ein sehr langer Name das
 * Badge-Layout sprengen (Risiko-/Entscheidungs-Badge in eine zweite Zeile
 * gedrückt). Kürzt auf {@link MCP_CLIENT_NAME_MAX_LENGTH} Zeichen. */
function truncateMcpClientName(name: string): string {
  const trimmed = name.trim();
  if (trimmed.length <= MCP_CLIENT_NAME_MAX_LENGTH) return trimmed;
  return `${trimmed.slice(0, MCP_CLIENT_NAME_MAX_LENGTH)}…`;
}

function decisionBadge(
  t: (key: string) => string,
  decision: Decision,
): { text: string; className: string } {
  if (decision === "AutoExec") {
    return { text: t("actionCard.autoExecuted"), className: "bg-emerald-900 text-emerald-300" };
  }
  if ("Confirm" in decision) {
    return { text: t("actionCard.confirmationNeeded"), className: "bg-amber-900 text-amber-300" };
  }
  return { text: t("actionCard.blocked"), className: "bg-red-900 text-red-300" };
}

/** Ampel-Kartenrahmen passend zur `Decision` (Spec 0009: Allow/Confirm/Deny
 * grün/gelb/rot) — Design-Import, Abschnitt "FARBSYSTEM · STATUS IST
 * ÜBERALL GLEICH CODIERT": derselbe Statuscode gilt für Kartenrahmen,
 * Badge und Dot durchgängig, nicht nur für die Badge wie zuvor. */
function decisionCardTone(decision: Decision): string {
  if (decision === "AutoExec") return "border-emerald-700/40 bg-emerald-950/20";
  if ("Confirm" in decision) return "border-amber-700/45 bg-amber-950/15";
  return "border-red-700/45 bg-red-950/15";
}

let nextId = 0;
const freshId = () => `item-${nextId++}`;

interface ChatPanelProps {
  sessionId: string;
  /** Spec 0011, Abschnitt 4: Default-Scope für den Regel-Schnellvorschlag
   * ist der aktuell verbundene Server. */
  serverId: string;
  /** Spec 0017, Abschnitt 5: informiert die Tab-Leiste (`useSessionTabs`),
   * sobald der Nutzer eine wartende Bestätigung auflöst — auch bei
   * Ablehnung, die (anders als Approve/EditThenApprove) kein
   * `chat-action-result`-Event auslöst und sonst den Hinweis-Indikator auf
   * dem Tab hängen ließe. */
  onActionSettled: (sessionId: string) => void;
}

export function ChatPanel({ sessionId, serverId, onActionSettled }: ChatPanelProps) {
  const [items, setItems] = useState<ChatItem[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  /** Spec 0021, Abschnitt 5: `true`, sobald eine *automatische* Folgerunde
   * läuft (KI reagiert auf ein Aktionsergebnis, ohne dass der Nutzer
   * getippt hat) — steuert den "Automatik läuft"-Indikator samt
   * "Automatik stoppen"-Button. Wird zusammen mit `sending` zurückgesetzt
   * (`handleSubmit`s `finally`): `send_chat_message` läuft synchron über
   * die komplette Runden-Kette, dessen Promise-Auflösung ist also das
   * zuverlässige "die ganze Kette ist zu Ende"-Signal, unabhängig davon, ob
   * sie regulär endete, am Sicherheits-Cap oder durch "Automatik stoppen". */
  const [autoContinuing, setAutoContinuing] = useState(false);
  const [hasActiveProvider, setHasActiveProvider] = useState<boolean | null>(null);
  /** Spec 0026, Abschnitt 4: nur bei aktivierter Zweitmeinung überhaupt
   * einen Lade-Indikator zeigen — sonst käme (bis ein `risk-assessment-
   * updated`-Event ausbliebe) fälschlich ein Spinner, der nie verschwindet.
   * Kann vom tatsächlichen Session-Verhalten abweichen, falls die
   * Einstellung erst NACH dem `connect()` dieser Session geändert wurde
   * (der Backend-Provider wird einmalig bei `connect()` aufgelöst, s.
   * `Session::risk_second_opinion_provider`-Doc-Kommentar) — ein bewusst
   * akzeptierter kleiner Randfall, kein Korrektheitsproblem. */
  const [riskSecondOpinionEnabled, setRiskSecondOpinionEnabled] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Spec 0015, Abschnitt 5: einmalig pro Server geladen, kein
  // wiederholtes Nachladen bei jeder Pfeiltaste. `historyNav` verfolgt den
  // laufenden Navigations-Modus (s. `promptHistoryNav.ts`) — beim
  // Serverwechsel zurückgesetzt, damit kein Navigations-Zustand auf die
  // History eines anderen Servers zeigt.
  const [promptHistory, setPromptHistory] = useState<string[]>([]);
  const [historyNav, setHistoryNav] = useState<HistoryNavState>(initialHistoryNavState);

  useEffect(() => {
    listPromptHistory(serverId)
      .then(setPromptHistory)
      .catch(() => setPromptHistory([]));
    setHistoryNav(initialHistoryNavState);
  }, [serverId]);

  useEffect(() => {
    listAiProviders()
      .then((providers) => setHasActiveProvider(providers.some((p) => p.isActive)))
      .catch(() => setHasActiveProvider(null));
  }, []);

  useEffect(() => {
    loadRiskClassifierSettings()
      .then((settings) => setRiskSecondOpinionEnabled(settings.enabled))
      .catch(() => setRiskSecondOpinionEnabled(false));
  }, []);

  // Spec 0034, Abschnitt 6/8: seedet den bereits geladenen Verlauf einer
  // fortgesetzten Sitzung (leer bei einem frischen `connect()`, s.
  // `get_chat_history`-Doc-Kommentar) — ohne das startet der Tab nach
  // einem "Fortsetzen" fälschlich leer, obwohl das Backend bereits mit
  // gefüllter Historie gestartet ist. Läuft einmal pro Tab (`sessionId`
  // ändert sich nie innerhalb desselben Tabs), vor jedem Live-Event.
  useEffect(() => {
    getChatHistory(sessionId)
      .then((entries) => {
        if (entries.length === 0) return;
        setItems((prev) => [
          ...entries.map((entry): ChatItem => {
            switch (entry.type) {
              case "text":
                return {
                  type: entry.role === "user" ? "user" : "assistant",
                  id: freshId(),
                  text: entry.text,
                };
              case "commandResult":
                return {
                  type: "historyCommandResult",
                  id: freshId(),
                  command: entry.command,
                  stdout: entry.stdout,
                  stderr: entry.stderr,
                  exitCode: entry.exitCode,
                  cancelled: entry.cancelled,
                };
              case "actionRejected":
                return {
                  type: "historyRejected",
                  id: freshId(),
                  command: entry.command,
                  reason: entry.reason,
                };
            }
          }),
          ...prev,
        ]);
      })
      .catch(() => {
        // Best-effort: eine fehlgeschlagene Historie-Ladung soll den Tab
        // nicht unbenutzbar machen, nur ohne sichtbaren Altverlauf starten
        // (derselbe Geist wie andere Best-effort-`.catch`-Stellen in
        // dieser Komponente, z. B. `listPromptHistory` oben).
      });
  }, [sessionId]);

  useEffect(() => {
    const unlisten = [
      onChatTextDelta((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => {
          const last = prev[prev.length - 1];
          if (last?.type === "assistant") {
            return [...prev.slice(0, -1), { ...last, text: last.text + event.delta }];
          }
          return [...prev, { type: "assistant", id: freshId(), text: event.delta }];
        });
      }),
      onChatActionProposed((event) => {
        if (event.sessionId !== sessionId) return;
        // Spec 0026, Abschnitt 3/4: eine Zweitmeinung ist nur zu erwarten,
        // wenn sie aktiviert ist UND die Regel-Einschätzung überhaupt noch
        // Raum nach oben hat (ein bereits regelbasiertes "red" kann nicht
        // weiter eskalieren, s. `escalate_data_risk` im Backend) — in
        // beiden Fällen bliebe der Spinner sonst ohne je eintreffendes
        // Update-Event hängen.
        const riskSecondOpinionPending =
          riskSecondOpinionEnabled &&
          event.riskAssessment !== null &&
          event.riskAssessment.dataRisk !== "red";
        setItems((prev) => [
          ...prev,
          {
            type: "action",
            id: freshId(),
            actionId: event.actionId,
            action: event.action,
            decision: event.decision,
            responded: false,
            previousNoteContent: event.previousNoteContent,
            usesStoredSudoPassword: event.usesStoredSudoPassword,
            previousFileContent: event.previousFileContent,
            previousFileSize: event.previousFileSize,
            targetName: event.targetName,
            riskAssessment: event.riskAssessment,
            riskSecondOpinionPending,
            // Spec 0027: bei AutoExec beginnt die Ausführung sofort (keine
            // Bestätigung nötig) — bei Confirm/Deny erst später, s.
            // `respond()` unten.
            startedAt: event.decision === "AutoExec" ? Date.now() : null,
            origin: event.origin,
          },
        ]);
      }),
      onRiskAssessmentUpdated((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) =>
          prev.map((item) =>
            item.type === "action" && item.actionId === event.actionId && item.riskAssessment
              ? {
                  ...item,
                  riskAssessment: {
                    ...item.riskAssessment,
                    dataRisk: event.dataRisk,
                    dataRiskReason: event.reason,
                    aiReviewed: true,
                  },
                  riskSecondOpinionPending: false,
                }
              : item,
          ),
        );
      }),
      onChatActionResult((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) =>
          prev.map((item) =>
            item.type === "action" && item.actionId === event.actionId
              ? { ...item, result: event.result, responded: true }
              : item,
          ),
        );
      }),
      onChatError((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [
          ...prev,
          { type: "error", id: freshId(), message: event.message, code: event.code ?? null },
        ]);
      }),
      onChatAutoContinuationLimitReached((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [
          ...prev,
          { type: "autoContinuationLimitReached", id: freshId(), limit: event.limit },
        ]);
      }),
      onChatAutoContinuationStarted((event) => {
        if (event.sessionId !== sessionId) return;
        setAutoContinuing(true);
      }),
      onChatDocumentGenerated((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [
          ...prev,
          {
            type: "document",
            id: freshId(),
            title: event.title,
            contentMarkdown: event.contentMarkdown,
          },
        ]);
      }),
    ];

    return () => {
      unlisten.forEach((p) => p.then((unlistenFn) => unlistenFn()));
    };
  }, [sessionId, riskSecondOpinionEnabled]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [items]);

  const respond = (actionId: string, decision: ActionUserDecision) => {
    // Spec 0027: die tatsächliche Ausführung beginnt jetzt (nicht bei
    // "deny", das führt nie etwas aus) — Startzeitpunkt für den
    // Lauf-Indikator.
    const startedAt =
      decision.decision === "approve" || decision.decision === "editThenApprove"
        ? Date.now()
        : null;
    setItems((prev) =>
      prev.map((item) =>
        item.type === "action" && item.actionId === actionId
          ? { ...item, responded: true, userDecision: decision.decision, startedAt: startedAt ?? item.startedAt }
          : item,
      ),
    );
    onActionSettled(sessionId);
    respondToAction(sessionId, actionId, decision).catch((err) =>
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err), code: null },
      ]),
    );
  };

  /** Spec 0021, Abschnitt 5: optimistisch sofort ausgeblendet — der
   * eigentliche Effekt (kein weiterer automatischer `send()`-Aufruf mehr)
   * greift erst beim nächsten Rundenübergang im Backend, aber der Indikator
   * soll nicht so lange weiter "läuft" suggerieren. */
  const handleStopAutoContinuation = () => {
    setAutoContinuing(false);
    stopAutoContinuation(sessionId).catch((err) =>
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err), code: null },
      ]),
    );
  };

  /** Spec 0011, Abschnitt 3: `accept_and_create_rule` legt die Regel an
   * **und** löst die Confirm-Entscheidung selbst auf (Backend) — anders als
   * `respond()` oben ruft dies also `respondToAction` nicht zusätzlich auf.
   *
   * `editedCommand`: unabhängiger Review-Pass (Spec 0007/0008/0011) —
   * `null`, falls der Text im Bearbeiten-Feld unverändert dem
   * KI-Vorschlag entspricht (Backend löst dann wie zuvor über `Approve`
   * auf); sonst der bearbeitete Text, damit das Backend über
   * `EditThenApprove` tatsächlich DIESEN Text erneut durch die
   * Filter-Engine schickt und ausführt — sonst würde bei jedem editierten
   * Kommando das ursprüngliche, unbearbeitete Kommando ausgeführt, während
   * die neue Regel aus dem bearbeiteten Text abgeleitet wird (genau der
   * Bestätigungsdialog-Bypass, gegen den `EditThenApprove` beim normalen
   * "Ausführen"-Button bereits schützt, s. `handleApprove` oben in
   * `ConfirmActionForm`).
   * `startedAt`: wie bei `respond()` oben — dieser Pfad führt immer aus
   * (nie "deny"), ohne das würde weder der Lauf-Indikator noch der
   * Abbrechen-Button (Spec 0027) für ein per Schnellregel akzeptiertes
   * Kommando erscheinen. */
  const acceptWithRule = (
    actionId: string,
    patternType: PatternType,
    patternValue: string,
    scope: Scope,
    editedCommand: string | null,
  ) => {
    setItems((prev) =>
      prev.map((item) =>
        item.type === "action" && item.actionId === actionId
          ? { ...item, responded: true, startedAt: Date.now() }
          : item,
      ),
    );
    onActionSettled(sessionId);
    acceptAndCreateRule(
      sessionId,
      actionId,
      patternType,
      patternValue,
      scope,
      undefined,
      editedCommand,
    ).catch((err) =>
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err), code: null },
      ]),
    );
  };

  /** Spec 0012, Abschnitt 3: der native Speichern-Dialog selbst lebt im
   * Backend (`export_document`) — bricht der Nutzer ihn ab, kehrt der
   * Command einfach ohne Fehler zurück (s. dortiger Doc-Kommentar), hier
   * ist dann schlicht nichts zu tun. */
  const handleExport = async (contentMarkdown: string, title: string, format: DocumentFormat) => {
    try {
      await exportDocument(contentMarkdown, title, format);
    } catch (err) {
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err), code: null },
      ]);
      throw err;
    }
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const text = draft.trim();
    if (!text || sending) return;
    setItems((prev) => [...prev, { type: "user", id: freshId(), text }]);
    setDraft("");
    setHistoryNav(initialHistoryNavState);
    setSending(true);
    try {
      await sendChatMessage(sessionId, text);
    } catch (err) {
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err), code: null },
      ]);
    } finally {
      // Spec 0021, Abschnitt 7: Fail-Safe — läuft in JEDEM Fall (Erfolg,
      // Fehler, gleich welcher der vier Ausgänge zuletzt griff), damit die
      // Eingabe nie dauerhaft gesperrt bleiben kann, selbst wenn die
      // automatische Fortsetzung selbst fehlschlägt (z. B. Netzwerkfehler
      // beim Folge-Request an den KI-Provider — landet als `chat-error`,
      // aber `sendChatMessage()` löst trotzdem auf, s. `run_chat_turn`).
      setSending(false);
      setAutoContinuing(false);
    }
  };

  // Spec 0015, Abschnitt 5: Pfeil-oben/-unten lösen Historien-Navigation nur
  // an den jeweiligen Feldrändern aus (Cursor-Position-Gate hier, reine
  // Index-Logik in `navigateHistory`) — sonst läuft die normale
  // Cursor-Bewegung des Browsers unverändert durch (kein `preventDefault`).
  const pendingCaretToEndRef = useRef(false);

  const handleInputKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    const input = e.currentTarget;
    const atStart = input.selectionStart === 0 && input.selectionEnd === 0;
    const atEnd =
      input.selectionStart === input.value.length && input.selectionEnd === input.value.length;
    if (e.key === "ArrowUp" && !atStart) return;
    if (e.key === "ArrowDown" && !atEnd) return;

    const result = navigateHistory(
      e.key === "ArrowUp" ? "up" : "down",
      promptHistory,
      historyNav,
      draft,
    );
    if (!result) return;

    e.preventDefault();
    setHistoryNav(result.nextState);
    setDraft(result.value);
    pendingCaretToEndRef.current = true;
  };

  // Setzt den Cursor nach dem Einsetzen eines Historieneintrags ans Ende
  // (Abschnitt 5: von der Spec nicht explizit vorgegeben, üblicher/
  // sinnvoller Default). Läuft absichtlich erst *nach* dem Commit des neuen
  // `draft`-Werts (statt synchron im Keydown-Handler) — nur dann steht der
  // tatsächliche DOM-Wert schon fest, gegen den `setSelectionRange`
  // rechnen muss. Der Ref-Flag verhindert, dass normales Tippen (das
  // `draft` ebenfalls ändert) den Cursor fälschlich ans Ende zwingt.
  useEffect(() => {
    if (pendingCaretToEndRef.current && inputRef.current) {
      const pos = inputRef.current.value.length;
      inputRef.current.setSelectionRange(pos, pos);
      pendingCaretToEndRef.current = false;
    }
  }, [draft]);

  const handleDraftChange = (value: string) => {
    setDraft(value);
    // Spec 0015, Abschnitt 5, bewusste MVP-Vereinfachung: jede normale
    // Texteingabe beendet den Navigations-Modus (kein volles
    // Readline-Verhalten). Feuert nie für die programmatischen
    // `setDraft`-Aufrufe aus `handleInputKeyDown` selbst — React-`onChange`
    // löst nur bei echten Browser-Eingabe-Events aus, nicht bei
    // State-Updates.
    if (historyNav.historyIndex !== null) {
      setHistoryNav(initialHistoryNavState);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div ref={scrollRef} className="flex-1 space-y-3 overflow-y-auto p-4">
        {items.map((item) => (
          <ChatItemView
            key={item.id}
            item={item}
            onRespond={respond}
            onAcceptWithRule={acceptWithRule}
            onExport={handleExport}
            serverId={serverId}
            sessionId={sessionId}
          />
        ))}
        {/* Spec 0021, Abschnitt 5: "Automatik läuft"-Indikator mit
         * Stopp-Möglichkeit hat Vorrang vor dem generischen
         * "generiert Antwort"-Hinweis — sobald eine automatische Folgerunde
         * läuft, ist das die genauere, handlungsfähigere Information. */}
        {autoContinuing ? (
          <div className="flex items-center gap-2 rounded-lg border border-indigo-700/50 bg-indigo-950/40 px-3 py-2 text-xs text-indigo-300">
            <span className="inline-block h-2 w-2 animate-ping rounded-full bg-indigo-400" />
            <span className="flex-1">
              🤖 Automatik läuft — KI reagiert automatisch auf das letzte Ergebnis…
            </span>
            <button
              type="button"
              onClick={handleStopAutoContinuation}
              className="font-heading border border-indigo-500/60 px-2 py-1 text-xs font-semibold tracking-wide text-indigo-200 hover:bg-indigo-600/20"
            >
              Automatik stoppen
            </button>
          </div>
        ) : (
          sending && (
            <div className="flex items-center gap-2 rounded-lg bg-slate-800/80 px-3 py-2 text-xs text-indigo-300">
              <span className="inline-block h-2 w-2 animate-ping rounded-full bg-indigo-400" />
              <span>KI generiert Antwort / Dokument…</span>
            </div>
          )
        )}
      </div>

      {hasActiveProvider === false ? (
        <div className="border-t border-slate-700 p-4 text-sm text-amber-300">
          Kein aktiver AI-Provider konfiguriert. Bitte zuerst in den Einstellungen einrichten.
        </div>
      ) : (
        <form onSubmit={handleSubmit} className="flex items-center gap-2 border-t border-slate-700 bg-slate-800 p-3">
          <span className="font-mono text-sm text-indigo-500">&gt;</span>
          <input
            ref={inputRef}
            type="text"
            value={draft}
            onChange={(e) => handleDraftChange(e.target.value)}
            onKeyDown={handleInputKeyDown}
            placeholder="Frage stellen oder Kommando beschreiben …"
            disabled={sending}
            className="flex-1 bg-transparent text-sm text-slate-100 placeholder:text-slate-500 focus:outline-none"
          />
          <button
            type="submit"
            disabled={sending || draft.trim().length === 0}
            className="font-heading bg-indigo-600 px-4 py-1.5 text-sm font-semibold tracking-wide text-slate-950 hover:bg-indigo-500 disabled:opacity-50"
          >
            Senden
          </button>
        </form>
      )}
    </div>
  );
}

const RISK_LEVEL_BADGE_CLASS: Record<Exclude<RiskAssessment["serverRisk"], "none">, string> = {
  yellow: "bg-amber-900 text-amber-300",
  red: "bg-red-900 text-red-300",
};

/** Spec 0026, Abschnitt 4: zwei getrennte Badges ("Server"/"Daten"), nur
 * sichtbar bei Level ≠ `none`, Tooltip mit der Begründung. Bewusst KEIN
 * drittes "grün"-Badge (s. Spec Abschnitt 1) — Abwesenheit heißt "laut
 * bekannten Mustern unauffällig", kein Sicherheitsversprechen (s.
 * `RiskHintFootnote` für den entsprechenden Text).
 *
 * Spec 0029: sitzt jetzt inline in derselben Zeile wie das Aktions-Label
 * und der Entscheidungs-Badge, statt als eigener Block über dem
 * Kommando-Text — deshalb kein eigenes `mb-*` mehr hier (das übernimmt die
 * Zeile als Ganzes), und `null` statt eines leeren Containers, wenn beide
 * Achsen `none` sind (kein Layout-Sprung, Spec Abschnitt 2). */
function RiskBadges({
  assessment,
  pending,
}: {
  assessment: RiskAssessment | null;
  pending: boolean;
}) {
  const { t } = useTranslation();
  if (!assessment) return null;

  const hasServerBadge = assessment.serverRisk !== "none";
  const hasDataBadge = assessment.dataRisk !== "none";
  if (!hasServerBadge && !hasDataBadge && !pending) return null;

  return (
    <span className="flex flex-wrap items-center gap-1.5">
      {hasServerBadge && (
        <span
          title={assessment.serverRiskReason ?? undefined}
          className={`font-heading px-1.5 py-0.5 text-[10px] font-semibold tracking-wide uppercase ${RISK_LEVEL_BADGE_CLASS[assessment.serverRisk as "yellow" | "red"]}`}
        >
          {t("actionCard.riskServerLabel")}
        </span>
      )}
      {hasDataBadge && (
        <span
          title={assessment.dataRiskReason ?? undefined}
          className={`font-heading px-1.5 py-0.5 text-[10px] font-semibold tracking-wide uppercase ${RISK_LEVEL_BADGE_CLASS[assessment.dataRisk as "yellow" | "red"]}`}
        >
          {t("actionCard.riskDataLabel")}
        </span>
      )}
      {pending && (
        <span
          title={t("actionCard.riskSecondOpinionPending")}
          className="inline-flex items-center gap-1 text-[10px] text-slate-500"
        >
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-slate-500" />
        </span>
      )}
    </span>
  );
}

/** Der zurückhaltende Fußnotentext aus der bisherigen `RiskBadges` — bleibt
 * eine eigene, unterhalb der Label-Zeile stehende Zeile (Spec 0029 regelt
 * nur die Position der Badges selbst, s. dortiger Abschnitt 2; der
 * vollständige Satz würde die Label-Zeile sprengen). `null`, wenn kein
 * Badge gezeigt wird — derselbe "kein leerer Platz"-Grundsatz wie bei
 * `RiskBadges`. */
function RiskHintFootnote({ assessment }: { assessment: RiskAssessment | null }) {
  const { t } = useTranslation();
  if (!assessment) return null;
  if (assessment.serverRisk === "none" && assessment.dataRisk === "none") return null;
  return <p className="mb-1 text-right text-[10px] text-slate-500">{t("actionCard.riskHint")}</p>;
}

// Exportiert für Komponententests (Spec 0029, Abschnitt 4: strukturelle
// DOM-Prüfung der Badge-Position) — sonst rein intern von `ChatPanel`
// genutzt.
export function ChatItemView({
  item,
  onRespond,
  onAcceptWithRule,
  onExport,
  serverId,
  sessionId,
}: {
  item: ChatItem;
  onRespond: (actionId: string, decision: ActionUserDecision) => void;
  onAcceptWithRule: (
    actionId: string,
    patternType: PatternType,
    patternValue: string,
    scope: Scope,
    editedCommand: string | null,
  ) => void;
  onExport: (contentMarkdown: string, title: string, format: DocumentFormat) => Promise<void>;
  serverId: string;
  sessionId: string;
}) {
  const { t } = useTranslation();
  if (item.type === "document") {
    return (
      <DocumentCard title={item.title} contentMarkdown={item.contentMarkdown} onExport={onExport} />
    );
  }
  if (item.type === "user") {
    return (
      <div className="ml-auto max-w-[80%] border border-indigo-500/60 bg-indigo-700 px-3 py-2 text-sm text-indigo-50">
        {item.text}
      </div>
    );
  }
  if (item.type === "assistant") {
    return <AssistantMessageView text={item.text} onExport={onExport} sessionId={sessionId} />;
  }
  if (item.type === "error") {
    return (
      <div className="border border-red-700/50 bg-red-950 px-3 py-2 text-sm text-red-300">
        ⚠ {translateErrorCode(t, item.code, item.message)}
      </div>
    );
  }
  if (item.type === "autoContinuationLimitReached") {
    // Spec 0021, Abschnitt 4 / ADR 0021: weicher Stopp, kein Fehler — eigener,
    // neutraler Ton statt der roten Fehler-Karte oben.
    return (
      <div className="border border-slate-600/50 bg-slate-800 px-3 py-2 text-sm text-slate-300">
        {t("actionCard.autoContinuationLimitReached", { limit: item.limit })}
      </div>
    );
  }
  if (item.type === "historyCommandResult") {
    const noteContent = [`$ ${item.command}`, item.stdout, item.stderr]
      .filter(Boolean)
      .join("\n");
    return (
      <div className="rounded border border-slate-700 bg-slate-900 px-3 py-2 text-xs">
        <div className="flex items-start justify-between gap-2">
          <p className="font-mono text-slate-300">$ {item.command}</p>
          <TakeIntoNoteButton sessionId={sessionId} content={noteContent} />
        </div>
        {item.cancelled && (
          <p className="mt-1 font-sans text-amber-300">{t("actionCard.commandCancelledNotice")}</p>
        )}
        {item.stdout && (
          <pre className="mt-1 whitespace-pre-wrap text-slate-400">{item.stdout}</pre>
        )}
        {item.stderr && <pre className="mt-1 whitespace-pre-wrap text-red-400">{item.stderr}</pre>}
        {!item.cancelled && (
          <p className="mt-1 text-slate-500">exit code: {item.exitCode ?? "—"}</p>
        )}
      </div>
    );
  }
  if (item.type === "historyRejected") {
    return (
      <div className="rounded border border-amber-800/50 bg-amber-950/30 px-3 py-2 text-xs text-amber-300">
        <p className="font-mono">$ {item.command}</p>
        <p className="mt-1">{item.reason}</p>
      </div>
    );
  }

  const { label, command } = describeAction(t, item.action, item.targetName);
  // Spec 0021, Abschnitt 6: eine vom Nutzer abgelehnte Aktion bleibt
  // sichtbar, statt zu verschwinden — `item.decision` selbst bleibt aber
  // weiterhin `Confirm{reason}` (das war die ursprüngliche Filter-Engine-
  // Entscheidung, nicht die Nutzerwahl), deshalb hier zusätzlich anhand von
  // `userDecision` erkannt und mit einem eigenen "abgelehnt"-Label/-Ton
  // überlagert, statt weiter fälschlich "Bestätigung nötig" zu zeigen.
  const rejectedByUser = item.responded && item.userDecision === "deny";
  const blockedByFilter = typeof item.decision === "object" && "Deny" in item.decision;
  const badge = rejectedByUser
    ? { text: t("actionCard.rejected"), className: "bg-red-900 text-red-300" }
    : decisionBadge(t, item.decision);
  const cardTone = rejectedByUser
    ? "border-red-700/45 bg-red-950/15"
    : decisionCardTone(item.decision);
  const needsConfirmation = !item.responded && typeof item.decision === "object" && "Confirm" in item.decision;

  return (
    <div className={`border p-3 text-sm ${cardTone}`}>
      {/* Spec 0029, Abschnitt 2: Label — Lücke — Risiko-Badges —
       * Entscheidungs-Badge, alle in derselben Zeile, der rechte Cluster
       * rechtsbündig ans Zeilenende (`ml-auto`). Der Ursprungs-Badge (Spec
       * 0028) ist in der Positionierungs-Spec nicht erwähnt, reiht sich
       * aber sinngemäß vor die Risiko-Badges ein — der Entscheidungs-Badge
       * bleibt so, wie von der Spec verlangt, das letzte Element. */}
      <div className="mb-1 flex flex-wrap items-center gap-2">
        <span className="font-heading font-semibold tracking-wide text-slate-100">{label}</span>
        <div className="ml-auto flex flex-wrap items-center justify-end gap-1.5">
          {item.origin.kind === "mcp" && (
            <span
              className="font-heading rounded-sm bg-purple-950 px-2 py-0.5 text-xs font-semibold tracking-wide text-purple-300"
              title={t("actionCard.mcpOriginHint")}
            >
              {t("actionCard.mcpOrigin", {
                // Unabhängiger Review-Pass (Spec 0028): `clientName` kommt
                // ungeprüft vom MCP-Client selbst (Handshake-`clientInfo.
                // name`) — ein böswilliger Client könnte sich z. B.
                // "Interner Chat" nennen. Der feste "Externes Tool (MCP)"-
                // Text im Template (s. locale-Datei) macht die Herkunft
                // unabhängig vom Namen erkennbar; die Kürzung verhindert
                // zusätzlich, dass ein überlanger Name das Badge-Layout
                // sprengt.
                name: truncateMcpClientName(
                  item.origin.clientName ?? t("actionCard.mcpOriginGeneric"),
                ),
              })}
            </span>
          )}
          <RiskBadges assessment={item.riskAssessment} pending={item.riskSecondOpinionPending} />
          <span
            className={`font-heading px-2 py-0.5 text-xs font-semibold tracking-wide uppercase ${badge.className}`}
          >
            {badge.text}
          </span>
        </div>
      </div>
      <RiskHintFootnote assessment={item.riskAssessment} />
      {command && (
        <code
          className={`block border border-slate-700 bg-slate-950 px-2 py-1 font-mono text-xs ${
            rejectedByUser || blockedByFilter
              ? "text-slate-500 line-through decoration-red-500/60"
              : "text-slate-200"
          }`}
        >
          {command}
        </code>
      )}
      {item.usesStoredSudoPassword && (
        <p className="mt-1 text-xs text-amber-300">{t("actionCard.usesStoredSudoPassword")}</p>
      )}
      {"ProposeNoteUpdate" in item.action && (
        <div className="mt-2">
          <NoteDiffPreview
            previousContent={item.previousNoteContent}
            newContent={item.action.ProposeNoteUpdate.new_content}
          />
        </div>
      )}
      {/* Spec 0020, Abschnitt 4.2, Punkt 5: dieselbe Diff-Komponente wie bei
       * `ProposeNoteUpdate` — außer bei einer Binärdatei (erkennbar an
       * `previousFileContent: null` UND `previousFileSize` gesetzt), dort
       * ein Größenvergleich-Hinweis statt eines (sinnlosen) Text-Diffs. */}
      {"WriteRemoteFile" in item.action && (
        <div className="mt-2">
          {item.previousFileContent === null && item.previousFileSize !== null ? (
            <BinaryFileChangeHint
              oldSize={item.previousFileSize}
              newContent={item.action.WriteRemoteFile.content}
            />
          ) : (
            <NoteDiffPreview
              previousContent={item.previousFileContent}
              newContent={item.action.WriteRemoteFile.content}
            />
          )}
        </div>
      )}
      {rejectedByUser ? (
        <p className="mt-1 text-xs text-red-300">{t("actionCard.rejectedByUser")}</p>
      ) : (
        <>
          {/* Unabhängiger Review-Pass (Spec 0024, Abschnitt 5): der stabile
           * `code` liegt im Payload bereits vor (s. `translateErrorCode`,
           * an anderen Stellen wie `FilterRulesView.tsx` schon verdrahtet),
           * wurde hier aber nie ausgewertet — genau die sichtbarste Stelle
           * (jeder Bestätigungsdialog) blieb dadurch hartcodiert deutsch.
           * Fällt auf den bisherigen (formatierten) Rohtext zurück, wenn
           * `code` unbekannt/nicht gesetzt ist — keine Regression. */}
          {typeof item.decision === "object" && "Confirm" in item.decision && (
            <p className="mt-1 text-xs text-slate-400">
              {translateErrorCode(
                t,
                item.decision.Confirm.code,
                formatReason(item.decision.Confirm.reason),
              )}
            </p>
          )}
          {typeof item.decision === "object" && "Deny" in item.decision && (
            <p className="mt-1 text-xs text-red-300">
              {translateErrorCode(t, item.decision.Deny.code, formatReason(item.decision.Deny.reason))}
            </p>
          )}
        </>
      )}

      {needsConfirmation && (
        <ConfirmActionForm
          actionId={item.actionId}
          initialCommand={command}
          onRespond={onRespond}
          onAcceptWithRule={onAcceptWithRule}
          serverId={serverId}
        />
      )}

      {/* Spec 0027, Abschnitt 2: nur für SuggestCommand relevant —
       * ReadRemoteFile/WriteRemoteFile laufen über SFTP, nicht über den
       * abbrechbaren Exec-Kanal (s. Spec, Abschnitt 5). */}
      {!item.result && "SuggestCommand" in item.action && item.startedAt !== null && (
        <RunningCommandIndicator actionId={item.actionId} startedAt={item.startedAt} />
      )}

      {item.result && <ActionResultView result={item.result} sessionId={sessionId} />}
    </div>
  );
}

/** Spec 0027, Abschnitt 2: erscheint erst 5 Sekunden nach `startedAt`, ohne
 * dass bis dahin ein Ergebnis eingetroffen ist — rein clientseitiger
 * Timer, kein Backend-Event nötig. Der Button trennt bewusst nur die
 * Verbindung zu *diesem* Kommando (s. `cancelRunningCommand`), nicht die
 * SSH-Verbindung/Session — deshalb absichtlich nicht "Kommando stoppen"
 * (suggeriert einen garantierten Kill, den es nicht gibt, s. Spec
 * Abschnitt 3, letzter Absatz). */
function RunningCommandIndicator({ actionId, startedAt }: { actionId: string; startedAt: number }) {
  const { t } = useTranslation();
  const [now, setNow] = useState(() => Date.now());
  const [cancelling, setCancelling] = useState(false);

  useEffect(() => {
    const interval = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(interval);
  }, []);

  const elapsedSeconds = Math.floor((now - startedAt) / 1000);
  if (elapsedSeconds < 5) return null;

  const handleCancel = () => {
    setCancelling(true);
    cancelRunningCommand(actionId).catch(() => setCancelling(false));
  };

  return (
    <div className="mt-2 flex items-center gap-2 border border-slate-700 bg-slate-950 px-2 py-1.5 text-xs text-slate-400">
      <span className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-amber-400" />
      <span className="flex-1">{t("actionCard.runningSince", { seconds: elapsedSeconds })}</span>
      <button
        type="button"
        onClick={handleCancel}
        disabled={cancelling}
        className="shrink-0 border border-slate-600 px-2 py-0.5 text-slate-300 hover:bg-slate-800 disabled:opacity-50"
      >
        {cancelling ? t("actionCard.cancellingCommand") : t("actionCard.cancelCommand")}
      </button>
    </div>
  );
}

function ConfirmActionForm({
  actionId,
  initialCommand,
  onRespond,
  onAcceptWithRule,
  serverId,
}: {
  actionId: string;
  initialCommand?: string;
  onRespond: (actionId: string, decision: ActionUserDecision) => void;
  onAcceptWithRule: (
    actionId: string,
    patternType: PatternType,
    patternValue: string,
    scope: Scope,
    editedCommand: string | null,
  ) => void;
  serverId: string;
}) {
  const { t } = useTranslation();
  const [edited, setEdited] = useState(initialCommand ?? "");

  const handleApprove = () => {
    if (initialCommand !== undefined && edited !== initialCommand) {
      onRespond(actionId, { decision: "editThenApprove", command: edited });
    } else {
      onRespond(actionId, { decision: "approve" });
    }
  };

  return (
    <div className="mt-3 space-y-3">
      {initialCommand !== undefined && (
        <div className="flex flex-col gap-1">
          <span className="font-heading text-xs font-semibold tracking-wide text-slate-400 uppercase">
            {t("confirmDialog.editableCommand")}
          </span>
          <textarea
            value={edited}
            onChange={(e) => setEdited(e.target.value)}
            rows={2}
            className="w-full border border-amber-700/50 bg-slate-950 px-3 py-2 font-mono text-sm text-slate-100"
          />
        </div>
      )}
      {/* Reihenfolge/Gruppierung aus dem Design-Import (Abschnitt 1b,
       * Bestätigungsdialog): primäre Aktion (Ausführen) links, Ablehnen
       * daneben, der Regel-Schnellvorschlag als eigene, deutlich
       * abgesetzte Aktionsgruppe rechts. */}
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={handleApprove}
          className="font-heading bg-emerald-600 px-4 py-1.5 text-xs font-semibold tracking-wide text-emerald-950 hover:bg-emerald-500"
        >
          {t("confirmDialog.execute")}
        </button>
        <button
          type="button"
          onClick={() => onRespond(actionId, { decision: "deny" })}
          className="font-heading border border-red-600/50 px-4 py-1.5 text-xs font-semibold tracking-wide text-red-400 hover:bg-red-600/12"
        >
          {t("confirmDialog.deny")}
        </button>
        {/* Spec 0011: nur für Kommando-Vorschläge sinnvoll, nicht für
         * Notiz-Aktualisierungen (kein `initialCommand` dort). */}
        {initialCommand !== undefined && (
          <QuickRuleButton
            actionId={actionId}
            command={edited}
            wasEdited={edited !== initialCommand}
            serverId={serverId}
            onAccept={onAcceptWithRule}
          />
        )}
      </div>
    </div>
  );
}

/** Spec 0011, Abschnitt 4: zusätzlicher Button neben Ausführen/Ablehnen,
 * öffnet ein kompaktes Dropdown mit Muster-Vorschlägen + Scope-Auswahl
 * (Default: aktueller Server). Klick auf einen Vorschlag ruft
 * `accept_and_create_rule` auf und schließt das Dropdown — der
 * Bestätigungsdialog selbst verschwindet dann über `onAccept`s
 * optimistisches `responded: true` (s. `ChatPanel.acceptWithRule`), genau
 * wie bei Ausführen/Ablehnen. */
function QuickRuleButton({
  actionId,
  command,
  wasEdited,
  serverId,
  onAccept,
}: {
  actionId: string;
  command: string;
  /** Unabhängiger Review-Pass (Spec 0007/0008/0011): `true`, wenn `command`
   * vom ursprünglichen KI-Vorschlag abweicht — steuert, ob beim Akzeptieren
   * `editedCommand` mitgeschickt wird (s. `onAccept`-Doc-Kommentar an
   * `ConfirmActionForm.onAcceptWithRule`). */
  wasEdited: boolean;
  serverId: string;
  onAccept: (
    actionId: string,
    patternType: PatternType,
    patternValue: string,
    scope: Scope,
    editedCommand: string | null,
  ) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [suggestions, setSuggestions] = useState<PatternSuggestionDto[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [scopeKind, setScopeKind] = useState<"server" | "global" | "tag">("server");
  const [tag, setTag] = useState("");

  // Unabhängiger Review-Pass: Vorschläge wurden bislang nur beim ERSTEN
  // Öffnen geladen und danach nie neu geholt — bearbeitet der Nutzer den
  // Text nach dem ersten Öffnen weiter, blieben die veralteten
  // Vorschläge (aus dem alten Text) stehen und ausgewählt.
  useEffect(() => {
    setSuggestions([]);
  }, [command]);

  const toggleOpen = () => {
    const next = !open;
    setOpen(next);
    if (next && suggestions.length === 0 && !loading) {
      setLoading(true);
      setError(null);
      suggestRulePatterns(command)
        .then(setSuggestions)
        .catch((err) => setError(commandErrorMessage(err)))
        .finally(() => setLoading(false));
    }
  };

  const buildScope = (): Scope | null => {
    if (scopeKind === "global") return "Global";
    if (scopeKind === "server") return { Server: serverId };
    const trimmed = tag.trim();
    return trimmed ? { Tag: trimmed } : null;
  };

  const handlePick = (suggestion: PatternSuggestionDto) => {
    const scope = buildScope();
    if (!scope) return;
    onAccept(
      actionId,
      suggestion.patternType,
      suggestion.patternValue,
      scope,
      wasEdited ? command : null,
    );
    setOpen(false);
  };

  return (
    <div className="relative ml-auto inline-flex border border-indigo-600/50">
      <button
        type="button"
        onClick={toggleOpen}
        className="font-heading px-3 py-1.5 text-xs font-semibold tracking-wide text-indigo-400 hover:bg-indigo-600/14"
      >
        {t("confirmDialog.quickRule")}
      </button>
      <button
        type="button"
        onClick={toggleOpen}
        aria-label={t("confirmDialog.quickRuleDropdownAria")}
        className="border-l border-indigo-600/40 px-2 font-mono text-xs text-indigo-400 hover:bg-indigo-600/14"
      >
        ▾
      </button>
      {open && (
        <div className="absolute right-0 top-full z-10 mt-1 w-72 space-y-2 border border-indigo-600/35 bg-slate-900 p-2 shadow-lg">
          <div className="font-heading border-b border-slate-800 pb-2 text-xs font-semibold tracking-wide text-slate-400 uppercase">
            {t("confirmDialog.newRulePatternHeading")}
          </div>
          <div className="flex gap-3 text-xs text-slate-300">
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={scopeKind === "server"}
                onChange={() => setScopeKind("server")}
              />
              {t("confirmDialog.scopeThisServer")}
            </label>
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={scopeKind === "global"}
                onChange={() => setScopeKind("global")}
              />
              {t("confirmDialog.scopeGlobal")}
            </label>
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={scopeKind === "tag"}
                onChange={() => setScopeKind("tag")}
              />
              {t("confirmDialog.scopeTag")}
            </label>
          </div>
          {scopeKind === "tag" && (
            <input
              type="text"
              value={tag}
              onChange={(e) => setTag(e.target.value)}
              placeholder={t("confirmDialog.tagNamePlaceholder")}
              className="w-full border border-slate-600 bg-slate-900 px-2 py-1 font-mono text-xs text-slate-100"
            />
          )}
          {loading && <p className="text-xs text-slate-400">{t("confirmDialog.loadingSuggestions")}</p>}
          {error && <p className="text-xs text-red-400">{error}</p>}
          {!loading && !error && suggestions.length === 0 && (
            <p className="text-xs text-slate-400">{t("confirmDialog.noSuggestions")}</p>
          )}
          <ul className="space-y-1">
            {suggestions.map((suggestion) => (
              <li key={suggestion.patternValue}>
                <button
                  type="button"
                  onClick={() => handlePick(suggestion)}
                  className="w-full px-2 py-1 text-left font-mono text-xs text-slate-200 hover:bg-indigo-600/14"
                >
                  {suggestion.label}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

/** Größenvergleich statt eines Text-Diffs für eine binäre Zieldatei (Spec
 * 0020, Abschnitt 4.2, Punkt 5, letzter Satz) — Byte-Länge in UTF-8, analog
 * zur serverseitigen Größenermittlung (dort ebenfalls Byte-, nicht
 * Zeichenlänge). */
function BinaryFileChangeHint({ oldSize, newContent }: { oldSize: number; newContent: string }) {
  const { t } = useTranslation();
  const newSize = new TextEncoder().encode(newContent).length;
  return (
    <p className="border border-slate-700 bg-slate-950 px-2 py-1.5 text-xs text-slate-400">
      {t("confirmDialog.binaryFileHint", {
        oldSize: formatBytes(oldSize),
        newSize: formatBytes(newSize),
      })}
    </p>
  );
}

function ActionResultView({
  result,
  sessionId,
}: {
  result: ActionResultPayload;
  sessionId: string;
}) {
  const { t } = useTranslation();
  if (result.kind === "noteUpdate") {
    return <p className="mt-2 text-xs text-emerald-300">{result.summary}</p>;
  }
  const truncate = (s: string, max = 2000) => (s.length > max ? `${s.slice(0, max)}\n… (gekürzt)` : s);
  if (result.kind === "fileRead") {
    return (
      <div className="mt-2 space-y-1 rounded bg-slate-950 p-2 font-mono text-xs">
        <div className="flex items-start justify-between gap-2 font-sans">
          <TakeIntoNoteButton sessionId={sessionId} content={result.content} />
        </div>
        <pre className="whitespace-pre-wrap text-slate-300">{truncate(result.content)}</pre>
      </div>
    );
  }
  if (result.kind === "fileWrite") {
    return (
      <div className="mt-2 space-y-1 text-xs text-emerald-300">
        <p>✓ Datei '{result.path}' geschrieben.</p>
        {result.backupPath && <p className="text-slate-400">Backup: {result.backupPath}</p>}
        {result.usedSudoPassword && (
          <p className="text-amber-300">🔑 Über hinterlegtes Sudo-Passwort geschrieben.</p>
        )}
      </div>
    );
  }
  const noteContent = [result.stdout, result.stderr].filter(Boolean).join("\n");
  return (
    <div className="mt-2 space-y-1 rounded bg-slate-950 p-2 font-mono text-xs">
      {/* Spec 0027, Abschnitt 4: bei Abbruch statt "exit code: —" ein
       * expliziter Hinweis — ein fehlender Exit-Code allein sähe sonst wie
       * eine Störung statt eines bewussten Nutzer-Abbruchs aus. */}
      {result.cancelled ? (
        <p className="font-sans text-amber-300">{t("actionCard.commandCancelledNotice")}</p>
      ) : null}
      {noteContent && (
        <div className="flex items-start justify-between gap-2 font-sans">
          <TakeIntoNoteButton sessionId={sessionId} content={noteContent} />
        </div>
      )}
      {result.stdout && (
        <pre className="whitespace-pre-wrap text-slate-300">{truncate(result.stdout)}</pre>
      )}
      {result.stderr && (
        <pre className="whitespace-pre-wrap text-red-300">{truncate(result.stderr)}</pre>
      )}
      {!result.cancelled && <p className="text-slate-500">exit code: {result.exitCode ?? "—"}</p>}
    </div>
  );
}

/** Spec 0040, Abschnitt 6: "In Notiz übernehmen" — startet über
 * `takeChatContentIntoNote` denselben `ProposeNoteUpdate`-Bestätigungsablauf
 * wie ein KI-Vorschlag (inkl. Diff-Vorschau), vorbefüllt mit `content`
 * dieser einen Zeile. Der neu vorgeschlagene Eintrag erscheint wie jeder
 * andere `chat-action-proposed`-Eintrag als eigene Karte im Chat-Verlauf
 * (`onChatActionProposed` in `ChatPanel`) — dieser Button selbst zeigt
 * keinen eigenen Dialog, nur einen kurzen Sende-/Fehler-Zustand. */
function TakeIntoNoteButton({ sessionId, content }: { sessionId: string; content: string }) {
  const { t } = useTranslation();
  const [state, setState] = useState<"idle" | "sending" | "error">("idle");

  const handleClick = () => {
    setState("sending");
    takeChatContentIntoNote(sessionId, content)
      .then(() => setState("idle"))
      .catch(() => setState("error"));
  };

  return (
    <button
      type="button"
      onClick={handleClick}
      disabled={state === "sending"}
      className="shrink-0 border border-slate-700 px-1.5 py-0.5 text-[11px] text-slate-400 hover:border-slate-500 hover:text-slate-200 disabled:opacity-50"
      title={t("actionCard.takeIntoNote")}
    >
      {state === "error" ? t("actionCard.takeIntoNoteFailed") : t("actionCard.takeIntoNote")}
    </button>
  );
}

/** Spec 0012, Abschnitt 3: eigene Karte für `chat-document-generated`,
 * gerendertes Markdown statt Rohtext, mit den beiden Export-Buttons. Kein
 * Bestätigungsdialog/keine Approve-Deny-Buttons wie bei `type: "action"` —
 * es ist bereits nichts Persistentes passiert, das rückgängig zu machen
 * wäre. */
function DocumentCard({
  title,
  contentMarkdown,
  onExport,
}: {
  title: string;
  contentMarkdown: string;
  onExport: (contentMarkdown: string, title: string, format: DocumentFormat) => Promise<void>;
}) {
  const [exporting, setExporting] = useState<DocumentFormat | null>(null);
  const [savedFormat, setSavedFormat] = useState<DocumentFormat | null>(null);

  const handleExportClick = async (format: DocumentFormat) => {
    setExporting(format);
    try {
      await onExport(contentMarkdown, title, format);
      setSavedFormat(format);
    } finally {
      setExporting(null);
    }
  };

  return (
    <div className="rounded-lg border border-indigo-700/60 bg-slate-800/80 p-3.5 text-sm shadow-md">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-base">📄</span>
          <span className="font-semibold text-slate-100">{title}</span>
        </div>
        <span className="rounded bg-indigo-950/80 px-2 py-0.5 text-xs text-indigo-300 border border-indigo-800/50">
          Dokument generiert
        </span>
      </div>
      <div className="prose prose-sm prose-invert max-w-none rounded bg-slate-950 p-3 prose-pre:bg-slate-900 prose-p:my-1 prose-ul:my-1 prose-ol:my-1 prose-headings:my-1.5">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{contentMarkdown}</ReactMarkdown>
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <button
          type="button"
          disabled={exporting !== null}
          onClick={() => handleExportClick("markdown")}
          className="rounded bg-indigo-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-600 disabled:opacity-50"
        >
          {exporting === "markdown" ? "Speichert…" : "Als Markdown speichern"}
        </button>
        <button
          type="button"
          disabled={exporting !== null}
          onClick={() => handleExportClick("word")}
          className="rounded bg-indigo-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-600 disabled:opacity-50"
        >
          {exporting === "word" ? "Speichert…" : "Als Word speichern (.docx)"}
        </button>
        {savedFormat && (
          <span className="text-xs text-emerald-400">
            ✓ Als {savedFormat === "word" ? "Word-Dokument" : "Markdown"} exportiert
          </span>
        )}
      </div>
    </div>
  );
}

function AssistantMessageView({
  text,
  onExport,
  sessionId,
}: {
  text: string;
  onExport: (contentMarkdown: string, title: string, format: DocumentFormat) => Promise<void>;
  sessionId: string;
}) {
  const [exporting, setExporting] = useState<DocumentFormat | null>(null);
  const [savedFormat, setSavedFormat] = useState<DocumentFormat | null>(null);

  const handleExportClick = async (format: DocumentFormat) => {
    setExporting(format);
    try {
      const firstLine =
        text
          .trim()
          .split("\n")[0]
          .replace(/^[#\s*-]+/, "")
          .slice(0, 40) || "Antwort";
      await onExport(text, firstLine, format);
      setSavedFormat(format);
    } finally {
      setExporting(null);
    }
  };

  return (
    <div className="max-w-[85%] space-y-2 rounded-lg bg-slate-800 p-3 text-sm text-slate-100 shadow-sm">
      <div className="prose prose-sm prose-invert max-w-none prose-pre:bg-slate-950 prose-p:my-1 prose-ul:my-1 prose-ol:my-1 prose-headings:my-1.5">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
      </div>
      <div className="flex flex-wrap items-center gap-2 border-t border-slate-700/60 pt-2 text-xs">
        <TakeIntoNoteButton sessionId={sessionId} content={text} />
        <span className="text-slate-400">Export:</span>
        <button
          type="button"
          disabled={exporting !== null}
          onClick={() => handleExportClick("markdown")}
          className="rounded bg-slate-700/80 px-2 py-1 text-xs text-slate-200 hover:bg-slate-600 hover:text-white disabled:opacity-50"
        >
          {exporting === "markdown" ? "Speichert…" : "📄 Als Markdown"}
        </button>
        <button
          type="button"
          disabled={exporting !== null}
          onClick={() => handleExportClick("word")}
          className="rounded bg-slate-700/80 px-2 py-1 text-xs text-slate-200 hover:bg-slate-600 hover:text-white disabled:opacity-50"
        >
          {exporting === "word" ? "Speichert…" : "📄 Als Word (.docx)"}
        </button>
        {savedFormat && (
          <span className="text-xs text-emerald-400">
            ✓ Als {savedFormat === "word" ? "Word" : "Markdown"} exportiert
          </span>
        )}
      </div>
    </div>
  );
}
