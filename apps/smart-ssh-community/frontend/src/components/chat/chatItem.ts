import { type ComposerAttachment } from "../../extensions/registry";
import type { ActionOrigin, ActionResultPayload, AiAction, AutoExecRule, Decision, RiskAssessment, WebActivityDto, QuestionOption } from "../../types";
import type { TranslateFn } from "../../i18nTypes";

export type ChatItem =
  | {
      type: "user";
      id: string;
      text: string;
      /** Issue #471: mitgesendete Anhänge (Name, Art). */
      attachments?: { name: string; kind: ComposerAttachment["kind"] }[];
      /** Spec 0066, §2: während einer laufenden Antwort gesendet — geht mit
       * der nächsten Anfrage an die KI mit (bis `chat-queued-messages-sent`). */
      queued?: boolean;
    }
  | {
      type: "assistant";
      id: string;
      text: string;
      /** Spec 0065, Teil 2: `true`, wenn genau diese Antwort durch das
       * Längenlimit abgeschnitten wurde (`chat-response-truncated`) — zeigt
       * einen Hinweis + „Weiter"-Knopf. Kein Inhalt/Text, ein reines
       * UI-Flag (Lehre aus Spec 0057: ein Hinweis *im* Text wäre von echter
       * Modellausgabe nicht unterscheidbar). */
      truncated?: boolean;
    }
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
      /** Issue #1019: the backend settled the confirmation without the
       * user (reconnect, timeout) or no longer knew it (`unknown`); the
       * card is final and has no buttons. */
      settled?: "reconnect" | "timeout" | "unknown";
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
      /** Issue #102: die KI-Zweitmeinung konnte nicht eingeholt werden
       * (aus `risk-assessment-updated`); die Karte zeigt einen Hinweis. */
      secondOpinionUnavailable: boolean;
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
      /** Issue #696: rules behind an automatic run (live only, not restored
       * from a saved chat). */
      autoExecRules?: AutoExecRule[];
    }
  | {
      type: "error";
      id: string;
      message: string;
      code: string | null;
      /** Issue #600 — provider details (backend-redacted) and whether
       * "Try again" is offered. */
      details?: {
        providerErrorType?: string | null;
        httpStatus?: number | null;
        providerMessage?: string | null;
        providerName?: string | null;
        timeoutSecs?: number | null;
        accessReturns?: string | null;
      };
      retryable?: boolean;
    }
  // Issue #596: a rule was created from the confirmation card but the
  // command will still ask next time; `reasons` are backend reason codes.
  | { type: "ruleNotice"; id: string; reasons: string[] }
  | { type: "contextCompacted"; id: string }
  | { type: "autoContinuationLimitReached"; id: string; limit: number }
  // Spec 0080, A3: eine Runde ohne Text und ohne vorgeschlagene Aktion
  // (`chat-response-empty`) — reines UI-Element aus dem Event, kein
  // Chat-Inhalt (dieselbe Begründung wie bei `responseCancelled`).
  | { type: "emptyResponse"; id: string }
  // Issue #647: the model declined the request (`chat-response-refused`).
  | { type: "refusedResponse"; id: string }
  // Spec 0105 §7 (Issue #169): das Provider-Konto hat die Web-Werkzeuge
  // abgeschaltet, die Antwort kommt ohne Web-Recherche — reines UI-Element
  // aus dem Event, fester Text ohne Fehlertext des Providers.
  | { type: "webResearchUnavailable"; id: string }
  // Spec 0066, §1: der Nutzer hat die laufende KI-Anfrage per Stopp
  // abgebrochen — reines UI-Element aus einem Event, nicht Teil des Texts.
  | { type: "responseCancelled"; id: string }
  // Spec 0061, Abschnitt 4: proaktives Warten vor dem nächsten KI-Aufruf,
  // rein informativ (kein Button) — dieselbe neutrale Karte wie
  // "autoContinuationLimitReached" oben.
  | { type: "aiBudgetWaiting"; id: string; waitSeconds: number }
  | { type: "document"; id: string; title: string; contentMarkdown: string }
  // Issue #547 (Spec 0111): the AI's question with answer options.
  | {
      type: "question";
      id: string;
      questionId: string;
      question: string;
      options: QuestionOption[];
      multiSelect: boolean;
    }
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
      truncated: boolean;
    }
  | { type: "historyRejected"; id: string; command: string; reason: string }
  // Issue #1020: a stored action result in plain text (not assistant text).
  | { type: "historyActionResult"; id: string; text: string }
  // Spec 0105: serverseitige Web-Recherche des Providers — live
  // (`chat-web-activity`) und aus einer fortgesetzten Historie gleich
  // dargestellt.
  | { type: "webActivity"; id: string; activity: WebActivityDto; activityId?: string }
  // Issue #471 (Spec 0110, §5): Anhänge einer fortgesetzten Nutzer-Nachricht.
  | {
      type: "historyAttachments";
      id: string;
      items: { name: string; kind: ComposerAttachment["kind"] }[];
    };

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
export function describeAction(
  t: TranslateFn,
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
export const INJECTION_REASON_MARKER = "\nReason: ";

/** Issue #693: splits the checker's reason off a confirmation reason of
 * code `FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM` (the backend appends it
 * after the last marker; the reason itself has no newline). */
export function splitInjectionReason(
  code: string | null | undefined,
  reason: string,
): { text: string; injectionReason: string | null } {
  if (code !== "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM") {
    return { text: reason, injectionReason: null };
  }
  const at = reason.lastIndexOf(INJECTION_REASON_MARKER);
  if (at < 0) return { text: reason, injectionReason: null };
  return {
    text: reason.slice(0, at),
    injectionReason: reason.slice(at + INJECTION_REASON_MARKER.length),
  };
}

export function formatReason(reason: string): string {
  return reason
    .split("; ")
    .filter((part, index, all) => all.indexOf(part) === index)
    .join(" · ");
}

export const MCP_CLIENT_NAME_MAX_LENGTH = 40;

/** Unabhängiger Review-Pass (Spec 0028): `clientName` ist ein vom
 * MCP-Client selbst gewählter, ungeprüfter String (Handshake-`clientInfo.
 * name`) — ohne Längenbegrenzung könnte ein sehr langer Name das
 * Badge-Layout sprengen (Risiko-/Entscheidungs-Badge in eine zweite Zeile
 * gedrückt). Kürzt auf {@link MCP_CLIENT_NAME_MAX_LENGTH} Zeichen. */
export function truncateMcpClientName(name: string): string {
  const trimmed = name.trim();
  if (trimmed.length <= MCP_CLIENT_NAME_MAX_LENGTH) return trimmed;
  return `${trimmed.slice(0, MCP_CLIENT_NAME_MAX_LENGTH)}…`;
}

export function decisionBadge(
  t: TranslateFn,
  decision: Decision,
): { text: string; className: string } {
  if (decision === "AutoExec") {
    return { text: t("actionCard.autoExecuted"), className: "bg-success-subtle-hover text-success-fg-soft" };
  }
  if ("Confirm" in decision) {
    return { text: t("actionCard.confirmationNeeded"), className: "bg-warning-subtle-hover text-warning-fg-soft" };
  }
  return { text: t("actionCard.blocked"), className: "bg-danger-subtle-hover text-danger-fg-soft" };
}

/** Ampel-Kartenrahmen passend zur `Decision` (Spec 0009: Allow/Confirm/Deny
 * grün/gelb/rot) — Design-Import, Abschnitt "FARBSYSTEM · STATUS IST
 * ÜBERALL GLEICH CODIERT": derselbe Statuscode gilt für Kartenrahmen,
 * Badge und Dot durchgängig, nicht nur für die Badge wie zuvor. */
export function decisionCardTone(decision: Decision): string {
  if (decision === "AutoExec") return "border-success-border-strong/40 bg-success-subtle/20";
  if ("Confirm" in decision) return "border-warning-border-strong/45 bg-warning-subtle/15";
  return "border-danger-border-strong/45 bg-danger-subtle/15";
}


