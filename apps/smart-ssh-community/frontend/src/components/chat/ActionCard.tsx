import { type ChatItem, describeAction, splitInjectionReason, formatReason, truncateMcpClientName, decisionBadge } from "./chatItem";
import { SegmentRules, useRuleStaging } from "./RuleStaging";
import { RiskBadges, RiskHintFootnote } from "./RiskBadges";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { cancelRunningCommand, takeChatContentIntoNote } from "../../api/api";
import { translateErrorCode } from "../../api/errorCodes";
import { type LiveOutput } from "../../lib/liveOutput";
import { LiveCommandOutput } from "../LiveCommandOutput";
import { formatBytes } from "../../lib/format";
import type { ActionResultPayload, ActionUserDecision, InjectionCheckState } from "../../api/types";
import { NoteDiffPreview } from "../NoteDiffPreview";
import { Button, Icon } from "../ui";
import { AutoExecRulesLine } from "../AutoExecRulesLine";
import { InjectionCheckStatus } from "../InjectionCheckStatus";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";
import { StatusSquare } from "../ui/StatusSquare";

export function ActionCard({
  item,
  liveOutput,
  waitingForInput = false,
  injectionCheck,
  onRespond,
  serverId,
  sessionId,
}: {
  item: Extract<ChatItem, { type: "action" }>;
  liveOutput?: LiveOutput;
  waitingForInput?: boolean;
  injectionCheck?: InjectionCheckState;
  onRespond: (actionId: string, decision: ActionUserDecision) => void;
  serverId: string;
  sessionId: string;
}) {
  const { t } = useTranslation();
  const { label, command } = describeAction(t, item.action, item.targetName);
  // Spec 0021, Abschnitt 6: eine vom Nutzer abgelehnte Aktion bleibt
  // sichtbar, statt zu verschwinden — `item.decision` selbst bleibt aber
  // weiterhin `Confirm{reason}` (das war die ursprüngliche Filter-Engine-
  // Entscheidung, nicht die Nutzerwahl), deshalb hier zusätzlich anhand von
  // `userDecision` erkannt und mit einem eigenen "abgelehnt"-Label/-Ton
  // überlagert, statt weiter fälschlich "Bestätigung nötig" zu zeigen.
  const rejectedByUser = item.responded && item.userDecision === "deny";
  const settled = item.settled;
  const blockedByFilter = typeof item.decision === "object" && "Deny" in item.decision;
  const badge = settled
    ? { text: t("actionCard.expired"), className: "text-danger-fg", icon: "x" as const }
    : rejectedByUser
    ? { text: t("actionCard.rejected"), className: "text-danger-fg", icon: "x" as const }
    : decisionBadge(t, item.decision);
  // Issue #1371: executed card shows how the command ran in its header
  // (bolt, "Ran automatically", rules, exit code); other states keep
  // their status colour in the header label and icon, with a neutral frame.
  const autoExecHeader = item.decision === "AutoExec" && !settled && !rejectedByUser;
  const exitCode =
    item.result && item.result.kind !== "noteUpdate" && item.result.kind !== "fileRead" &&
    item.result.kind !== "fileWrite" && !item.result.cancelled
      ? (item.result.exitCode ?? "—")
      : null;
  const needsConfirmation = !item.responded && typeof item.decision === "object" && "Confirm" in item.decision;

  return (
    <div className="border border-border bg-surface-sunken text-sm">
      {/* Spec 0029, Abschnitt 2: Label — Lücke — Risiko-Badges —
       * Entscheidungs-Badge, alle in derselben Zeile, der rechte Cluster
       * rechtsbündig ans Zeilenende (`ml-auto`). Der Ursprungs-Badge (Spec
       * 0028) ist in der Positionierungs-Spec nicht erwähnt, reiht sich
       * aber sinngemäß vor die Risiko-Badges ein — der Entscheidungs-Badge
       * bleibt so, wie von der Spec verlangt, das letzte Element. */}
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-2">
        {autoExecHeader ? (
          <AutoExecRulesLine rules={item.autoExecRules ?? []} />
        ) : (
          <span className="text-label text-fg">{label}</span>
        )}
        <div className="ml-auto flex flex-wrap items-center justify-end gap-1.5">
          {item.origin.kind === "mcp" && (
            <span
              className="font-heading bg-mcp-subtle px-2 py-0.5 text-xs font-semibold tracking-wide text-mcp-fg"
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
          <RiskBadges
            assessment={item.riskAssessment}
            pending={item.riskSecondOpinionPending}
            unavailable={item.secondOpinionUnavailable}
          />
          {!autoExecHeader && (
            <span className={`text-label inline-flex items-center gap-1 ${badge.className}`}>
              <Icon name={badge.icon} />
              {badge.text}
            </span>
          )}
          {autoExecHeader && exitCode !== null && (
            <span className="font-mono text-xs text-fg-subtle">exit code: {exitCode}</span>
          )}
        </div>
      </div>
      <div className="p-3">
      <RiskHintFootnote assessment={item.riskAssessment} />
      {command && (
        <code
          className={`block font-mono text-xs break-words whitespace-pre-wrap before:content-['$_'] ${
            settled || rejectedByUser || blockedByFilter
              ? "text-fg-subtle line-through decoration-danger-strong/60"
              : "text-fg-soft"
          }`}
        >
          {command}
        </code>
      )}
      {item.usesStoredSudoPassword && (
        // Spec 0068, Teil 3: beim Schreiben ist Sudo ein möglicher
        // Fallback nach einem Rechte-Fehler — vorab angekündigt, nie still.
        <p className="mt-1 text-xs text-warning-fg-soft">
          <Icon name="key" className="mr-1 inline align-text-bottom" />
          {"WriteRemoteFile" in item.action
            ? t("actionCard.writeMayUseStoredSudoPassword")
            : t("actionCard.usesStoredSudoPassword")}
        </p>
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
      {settled ? (
        <output className="mt-1 block text-xs text-danger-fg-soft">
          {t(
            settled === "reconnect"
              ? "actionCard.discardedAfterReconnect"
              : settled === "timeout"
                ? "actionCard.expiredTimeout"
                : "actionCard.expiredUnknown",
          )}
        </output>
      ) : rejectedByUser ? (
        <p className="mt-1 text-xs text-danger-fg-soft">{t("actionCard.rejectedByUser")}</p>
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
            <p className="mt-1 text-xs text-fg-subtle">
              {(() => {
                // Issue #693: the injection-suspicion confirmation carries the
                // checking AI's reason after a fixed marker; shown as plain text.
                const { text, injectionReason } = splitInjectionReason(
                  item.decision.Confirm.code,
                  item.decision.Confirm.reason,
                );
                return (
                  <>
                    {translateErrorCode(t, item.decision.Confirm.code, formatReason(text))}
                    {injectionReason !== null && (
                      <span className="mt-0.5 block" data-testid="injection-confirm-reason">
                        {t("chat.injectionConfirmReason", { reason: injectionReason })}
                      </span>
                    )}
                  </>
                );
              })()}
            </p>
          )}
          {typeof item.decision === "object" && "Deny" in item.decision && (
            <p className="mt-1 text-xs text-danger-fg-soft">
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
                    serverId={serverId}
          sessionId={sessionId}
        />
      )}

      {/* Spec 0027, Abschnitt 2: nur für SuggestCommand relevant —
       * ReadRemoteFile/WriteRemoteFile laufen über SFTP, nicht über den
       * abbrechbaren Exec-Kanal (s. Spec, Abschnitt 5). */}
      {!item.result && "SuggestCommand" in item.action && item.startedAt !== null && (
        <RunningCommandIndicator
          actionId={item.actionId}
          startedAt={item.startedAt}
          waitingForInput={waitingForInput}
        />
      )}
      {!item.result && "SuggestCommand" in item.action && liveOutput && (
        <LiveCommandOutput output={liveOutput} />
      )}

      {item.result && <ActionResultView result={item.result} sessionId={sessionId} exitInHeader={autoExecHeader} />}
      {item.result && <InjectionCheckStatus check={injectionCheck} />}
      </div>
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
export function RunningCommandIndicator({
  actionId,
  startedAt,
  waitingForInput,
}: {
  actionId: string;
  startedAt: number;
  waitingForInput: boolean;
}) {
  const { t } = useTranslation();
  const [now, setNow] = useState(() => Date.now());
  const [cancelling, setCancelling] = useState(false);

  useEffect(() => {
    const interval = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(interval);
  }, []);

  const elapsedSeconds = Math.floor((now - startedAt) / 1000);
  // Issue #327: the stop button is available immediately while the command
  // is probably waiting for input.
  if (elapsedSeconds < 5 && !waitingForInput) return null;

  const handleCancel = () => {
    setCancelling(true);
    cancelRunningCommand(actionId).catch(() => setCancelling(false));
  };

  return (
    <div className="mt-2 flex items-center gap-2 border border-border bg-surface-sunken px-2 py-1.5 text-xs text-fg-subtle">
      <StatusSquare tone="warning" pulse />
      <span className="flex-1">
        {waitingForInput
          ? t("actionCard.waitingForInputHint")
          : t("actionCard.runningSince", { seconds: elapsedSeconds })}
      </span>
      <Button size="sm" className="shrink-0" onClick={handleCancel} disabled={cancelling}>
        {cancelling ? t("actionCard.cancellingCommand") : t("actionCard.cancelCommand")}
      </Button>
    </div>
  );
}

export function ConfirmActionForm({
  actionId,
  initialCommand,
  onRespond,
  serverId,
  sessionId,
}: {
  actionId: string;
  sessionId: string;
  initialCommand?: string;
  onRespond: (actionId: string, decision: ActionUserDecision) => void;
  serverId: string;
}) {
  const { t } = useTranslation();
  const [edited, setEdited] = useState(initialCommand ?? "");
  const staging = useRuleStaging(sessionId, edited, initialCommand !== undefined);

  const handleApprove = () => {
    if (initialCommand !== undefined && edited !== initialCommand) {
      onRespond(actionId, { decision: "editThenApprove", command: edited });
    } else {
      onRespond(actionId, { decision: "approve" });
    }
  };

  const canSaveRules =
    initialCommand !== undefined &&
    (staging.explanation?.segments.some((s) => s.state === "needsRule") ?? false);
  return (
    <div className="mt-3 space-y-3">
      {initialCommand !== undefined && (
        // Issue #166: a `label`, not a `div` — the caption is the field's
        // accessible name (without it screen readers announce an unnamed
        // text field in the confirmation dialog).
        <label className="flex flex-col gap-1">
          <span className="text-label text-fg-subtle">
            {t("confirmDialog.editableCommand")}
          </span>
          <textarea
            {...TECHNICAL_INPUT_PROPS}
            value={edited}
            onChange={(e) => setEdited(e.target.value)}
            rows={2}
            className="w-full border border-warning-border-strong/50 bg-surface-sunken px-3 py-2 font-mono text-sm break-all text-fg"
          />
        </label>
      )}
      {initialCommand !== undefined && (
        <SegmentRules staging={staging} sessionId={sessionId} command={edited} serverId={serverId} />
      )}
      <div className="flex flex-wrap items-center gap-2 border-t border-border pt-3">
        {canSaveRules && (
          <Button
            size="sm"
            className="narrow:min-h-11"
            disabled={staging.unsavedCount < 1 || staging.saving}
            onClick={staging.save}
          >
            <Icon name="shield" className="mr-1 inline align-text-bottom" />
            {t("confirmDialog.explain.saveRules", { count: staging.unsavedCount })}
          </Button>
        )}
        {/* Issue #1357: unsaved staged rules are only a hint; Execute does
         * not save them, Deny and Execute discard them. */}
        {staging.unsavedCount > 0 && (
          <span className="text-xs text-warning-fg-soft">
            {t("confirmDialog.explain.unsavedHint", { count: staging.unsavedCount })}
          </span>
        )}
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <Button
            variant="danger"
            size="sm"
            className="px-4 py-1.5 narrow:min-h-11"
            onClick={() => onRespond(actionId, { decision: "deny" })}
          >
            <Icon name="x" className="mr-1 inline align-text-bottom" />
            {t("confirmDialog.deny")}
          </Button>
          {/* Allow colour (Spec 0009): no shared variant, so the policy-decision
           * look stays hand-written; the focus ring is the shared one. */}
          <Button variant="bare"
            type="button"
            onClick={handleApprove}
            className="text-label bg-success px-4 py-1.5 narrow:min-h-11 text-success-subtle hover:bg-success-strong focus-ring"
          >
            <Icon name="play" className="mr-1 inline align-text-bottom" />
            {t("confirmDialog.execute")}
          </Button>
        </div>
      </div>
    </div>
  );
}

/** Größenvergleich statt eines Text-Diffs für eine binäre Zieldatei (Spec
 * 0020, Abschnitt 4.2, Punkt 5, letzter Satz) — Byte-Länge in UTF-8, analog
 * zur serverseitigen Größenermittlung (dort ebenfalls Byte-, nicht
 * Zeichenlänge). */
export function BinaryFileChangeHint({ oldSize, newContent }: { oldSize: number; newContent: string }) {
  const { t } = useTranslation();
  const newSize = new TextEncoder().encode(newContent).length;
  return (
    <p className="border border-border bg-surface-sunken px-2 py-1.5 text-xs text-fg-subtle">
      <Icon name="warn" className="mr-1 inline align-text-bottom" />
      {t("confirmDialog.binaryFileHint", {
        oldSize: formatBytes(oldSize),
        newSize: formatBytes(newSize),
      })}
    </p>
  );
}

/** Finished output longer than this many lines is shown as a preview. */
export const OUTPUT_PREVIEW_LINES = 8;

/** Output block with an 8-line preview and a per-card "Show all" toggle (not persisted). */
export function CollapsibleOutput({ text, className }: { text: string; className: string }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const lines = text.split("\n");
  const hidden = lines.length - OUTPUT_PREVIEW_LINES;
  if (hidden <= 0) {
    return <pre className={`whitespace-pre-wrap ${className}`}>{text}</pre>;
  }
  return (
    <div>
      <pre className={`whitespace-pre-wrap ${className}`}>
        {expanded ? text : lines.slice(0, OUTPUT_PREVIEW_LINES).join("\n")}
      </pre>
      {!expanded && <p className="text-fg-subtle">{t("chat.outputMoreLines", { n: hidden })}</p>}
      <button
        type="button"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
        className="font-sans text-xs text-accent-fg underline"
      >
        {expanded ? t("chat.outputShowLess") : t("chat.outputShowAll")}
      </button>
    </div>
  );
}

export function ActionResultView({
  result,
  sessionId,
  exitInHeader = false,
}: {
  result: ActionResultPayload;
  sessionId: string;
  exitInHeader?: boolean;
}) {
  const { t } = useTranslation();
  if (result.kind === "noteUpdate") {
    return <p className="mt-2 text-xs text-success-fg-soft">{result.summary}</p>;
  }
  const truncate = (s: string, max = 2000) =>
    s.length > max ? `${s.slice(0, max)}\n… ${t("chat.outputTruncated")}` : s;
  if (result.kind === "fileRead") {
    return (
      <div className="mt-2 space-y-1 border-t border-border pt-2 font-mono text-xs">
        <div className="flex items-start justify-between gap-2 font-sans">
          <TakeIntoNoteButton sessionId={sessionId} content={result.content} />
        </div>
        <pre className="whitespace-pre-wrap text-fg-muted">{truncate(result.content)}</pre>
      </div>
    );
  }
  if (result.kind === "fileWrite") {
    return (
      <div className="mt-2 space-y-1 text-xs text-success-fg-soft">
        <p><Icon name="check" className="mr-1 inline align-text-bottom" /> {t("chat.fileWritten", { path: result.path })}</p>
        {result.backupPath && <p className="text-fg-subtle">Backup: {result.backupPath}</p>}
        {result.usedSudoPassword && (
          <p className="text-warning-fg-soft"><Icon name="key" className="mr-1 inline align-text-bottom" /> {t("chat.writtenWithSudoPassword")}</p>
        )}
      </div>
    );
  }
  const noteContent = [result.stdout, result.stderr].filter(Boolean).join("\n");
  return (
    <div className="mt-2 space-y-1 border-t border-border pt-2 font-mono text-xs">
      {/* Spec 0027, Abschnitt 4: bei Abbruch statt "exit code: —" ein
       * expliziter Hinweis — ein fehlender Exit-Code allein sähe sonst wie
       * eine Störung statt eines bewussten Nutzer-Abbruchs aus. */}
      {result.cancelled ? (
        <p className="font-sans text-warning-fg-soft"><Icon name="warn" className="mr-1 inline align-text-bottom" />{t("actionCard.commandCancelledNotice")}</p>
      ) : null}
      {result.truncated ? (
        <p className="font-sans text-warning-fg-soft"><Icon name="warn" className="mr-1 inline align-text-bottom" />{t("actionCard.commandTruncatedNotice")}</p>
      ) : null}
      {noteContent && (
        <div className="flex items-start justify-between gap-2 font-sans">
          <TakeIntoNoteButton sessionId={sessionId} content={noteContent} />
        </div>
      )}
      {result.stdout && (
        <CollapsibleOutput text={truncate(result.stdout)} className="text-fg-subtle" />
      )}
      {result.stderr && (
        <CollapsibleOutput text={truncate(result.stderr)} className="text-danger-fg-soft" />
      )}
      {!result.cancelled && !exitInHeader && <p className="text-fg-subtle">exit code: {result.exitCode ?? "—"}</p>}
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
export function TakeIntoNoteButton({
  sessionId,
  content,
  onStateChange,
}: {
  sessionId: string;
  content: string;
  /** Spec-Reviewer-Fund (Spec 0055, Review des Gesamtpakets): optional,
   * damit `AssistantMessageView` seine hover-ausgeblendete Aktionsleiste
   * bei einem laufenden/fehlgeschlagenen Versuch erzwungen sichtbar
   * halten kann (s. dortiger Kommentar) — der unveränderte Aufrufer bei
   * `historyCommandResult`-Einträgen (immer sichtbar, kein Hover-
   * Ausblenden) braucht das nicht und lässt es einfach weg. */
  onStateChange?: (state: "idle" | "sending" | "error") => void;
}) {
  const { t } = useTranslation();
  const [state, setState] = useState<"idle" | "sending" | "error">("idle");

  const handleClick = () => {
    setState("sending");
    onStateChange?.("sending");
    takeChatContentIntoNote(sessionId, content)
      .then(() => {
        setState("idle");
        onStateChange?.("idle");
      })
      .catch(() => {
        setState("error");
        onStateChange?.("error");
      });
  };

  return (
    <Button
      size="sm"
      className="shrink-0"
      onClick={handleClick}
      disabled={state === "sending"}
      title={t("actionCard.takeIntoNote")}
    >
      {state === "error" ? t("actionCard.takeIntoNoteFailed") : t("actionCard.takeIntoNote")}
    </Button>
  );
}

