import { memo } from "react";
import { type ChatItem } from "./chatItem";
import { ActionCard, TakeIntoNoteButton } from "./ActionCard";
import { AssistantMessageView } from "./AssistantMessageView";
import { errorCardText, ErrorCardExtras } from "./ErrorCard";
import { reasonList } from "./CoverageList";
import { useTranslation } from "react-i18next";
import { type LiveOutput } from "../../liveOutput";
import { type ComposerAttachment } from "../../extensions/registry";
import type { ActionUserDecision, DocumentFormat, SuggestionVerdictDto, SuggestionPatternDto, Scope, InjectionCheckState } from "../../types";
import { DocumentCard } from "./DocumentCard";
import { WebActivityCard } from "../WebActivityCard";
import { InjectionCheckStatus } from "../InjectionCheckStatus";
import { QuestionCard, type QuestionState } from "../QuestionCard";

export const ChatItemView = memo(function ChatItemView({
  item,
  liveOutput,
  waitingForInput = false,
  injectionCheck,
  onRespond,
  onAcceptWithRule,
  onExport,
  onContinueTruncated,
  onRetryTurn,
  retryDisabled = false,
  questionState,
  onQuestionAnswer,
  serverId,
  sessionId,
}: {
  item: ChatItem;
  /** Issue #325: live output while the command runs (`undefined` before
   * the first output and after the result). */
  liveOutput?: LiveOutput;
  /** Issue #327: the backend marked the running command as probably
   * waiting for input. */
  waitingForInput?: boolean;
  /** Issue #693: state of the injection check of this item, if any. */
  injectionCheck?: InjectionCheckState;
  onRespond: (actionId: string, decision: ActionUserDecision) => void;
  onAcceptWithRule: (
    actionId: string,
    patterns: SuggestionPatternDto[],
    scope: Scope,
    editedCommand: string | null,
    verdict?: SuggestionVerdictDto,
  ) => void;
  onExport: (contentMarkdown: string, title: string, format: DocumentFormat) => Promise<void>;
  /** Spec 0065, Teil 2 — s. `ChatPanel`s `handleContinueTruncated`-Kommentar. */
  onContinueTruncated: (itemId: string) => void;
  /** Issue #600 — "Try again" on a retryable error card. */
  onRetryTurn?: (itemId: string) => void;
  retryDisabled?: boolean;
  /** Issue #547: derived lock state / answer handler of a question card. */
  questionState?: QuestionState;
  onQuestionAnswer?: (text: string) => void;
  serverId: string;
  sessionId: string;
}) {
  const { t } = useTranslation();
  if (item.type === "question") {
    return (
      <QuestionCard
        question={item.question}
        options={item.options}
        multiSelect={item.multiSelect}
        state={questionState ?? { locked: true, selected: [], ownWords: false }}
        onAnswer={onQuestionAnswer ?? (() => {})}
      />
    );
  }
  if (item.type === "document") {
    return (
      <DocumentCard title={item.title} contentMarkdown={item.contentMarkdown} onExport={onExport} />
    );
  }
  if (item.type === "user") {
    return (
      <div className="ml-auto max-w-[80%] narrow:max-w-[95%] break-words border border-accent-strong/60 bg-accent-border-strong px-3 py-2 text-sm text-accent-fg-faintest">
        {item.text}
        {item.attachments && item.attachments.length > 0 && (
          <p className="mt-1 text-xs text-accent-fg-softer/80">
            {t("chat.historyAttached", { files: attachmentLine(t, item.attachments) })}
          </p>
        )}
        {item.queued && (
          <p className="mt-1 text-xs text-accent-fg-softer/80">⏳ {t("chat.queued")}</p>
        )}
      </div>
    );
  }
  if (item.type === "assistant") {
    return (
      <AssistantMessageView
        text={item.text}
        onExport={onExport}
        sessionId={sessionId}
        truncated={item.truncated ?? false}
        onContinue={() => onContinueTruncated(item.id)}
      />
    );
  }
  if (item.type === "error") {
    return (
      <div className="border border-danger-border-strong/50 bg-danger-subtle px-3 py-2 text-sm text-danger-fg-soft">
        ⚠ {errorCardText(t, item)}
        {(item.details || item.retryable) && (
          <ErrorCardExtras item={item} onRetryTurn={onRetryTurn} retryDisabled={retryDisabled} />
        )}
      </div>
    );
  }
  if (item.type === "ruleNotice") {
    return (
      <output className="block border border-border-strong/50 bg-surface-raised px-3 py-2 text-sm text-fg-muted">
        {t("confirmDialog.explain.notice", { reasons: reasonList(t, item.reasons) })}
      </output>
    );
  }
  if (item.type === "autoContinuationLimitReached") {
    // Spec 0021, Abschnitt 4 / ADR 0021: weicher Stopp, kein Fehler — eigener,
    // neutraler Ton statt der roten Fehler-Karte oben.
    return (
      <div className="border border-border-strong/50 bg-surface-raised px-3 py-2 text-sm text-fg-muted">
        {t("actionCard.autoContinuationLimitReached", { limit: item.limit })}
      </div>
    );
  }
  if (item.type === "contextCompacted") {
    return <div className="px-1 text-xs text-fg-subtle">{t("chat.contextCompactedNotice")}</div>;
  }
  if (item.type === "responseCancelled") {
    return <div className="px-1 text-xs text-fg-subtle">⏹ {t("chat.responseCancelled")}</div>;
  }
  if (item.type === "emptyResponse") {
    // Spec 0080, A3: kein „Weiter"-Knopf (der Fortsetzungstext meint eine
    // abgeschnittene Antwort, das passt hier nicht) und keine
    // Export-/Notiz-Leiste (es gibt nichts zu exportieren) — anders als die
    // Kürzungs-Karte unten (`AssistantMessageView`, `truncated`) bewusst
    // kein eigenständiges Element mit Aktionen, nur ein Hinweis.
    return (
      <div className="border border-warning-border-strong/40 bg-surface-raised px-3 py-2 text-sm text-warning-fg-softer">
        {t("chat.emptyResponse")}
      </div>
    );
  }
  if (item.type === "refusedResponse") {
    return (
      <div className="border border-warning-border-strong/40 bg-surface-raised px-3 py-2 text-sm text-warning-fg-softer">
        {t("chat.refusedResponse")}
      </div>
    );
  }
  if (item.type === "webResearchUnavailable") {
    // Spec 0105 §7: Hinweis, kein Fehler — die Antwort folgt ohne
    // Web-Recherche. Neutraler Ton wie bei "autoContinuationLimitReached".
    return (
      <div className="border border-border-strong/50 bg-surface-raised px-3 py-2 text-sm text-fg-muted">
        {t("chat.webResearchUnavailable")}
      </div>
    );
  }
  if (item.type === "aiBudgetWaiting") {
    // Spec 0061, Abschnitt 4: rein informativ, kein Button — der Request
    // geht nach `waitSeconds` automatisch raus.
    return (
      <div className="border border-border-strong/50 bg-surface-raised px-3 py-2 text-sm text-fg-muted">
        {t("actionCard.aiBudgetWaiting", { seconds: item.waitSeconds })}
      </div>
    );
  }
  if (item.type === "historyCommandResult") {
    const noteContent = [`$ ${item.command}`, item.stdout, item.stderr]
      .filter(Boolean)
      .join("\n");
    return (
      <div className="rounded border border-border bg-surface px-3 py-2 text-xs">
        <div className="flex items-start justify-between gap-2">
          <p className="font-mono text-fg-muted">$ {item.command}</p>
          <TakeIntoNoteButton sessionId={sessionId} content={noteContent} />
        </div>
        {item.cancelled && (
          <p className="mt-1 font-sans text-warning-fg-soft">{t("actionCard.commandCancelledNotice")}</p>
        )}
        {item.truncated && (
          <p className="mt-1 font-sans text-warning-fg-soft">{t("actionCard.commandTruncatedNotice")}</p>
        )}
        {item.stdout && (
          <pre className="mt-1 whitespace-pre-wrap text-fg-subtle">{item.stdout}</pre>
        )}
        {item.stderr && <pre className="mt-1 whitespace-pre-wrap text-danger-fg">{item.stderr}</pre>}
        {!item.cancelled && (
          <p className="mt-1 text-fg-subtle">exit code: {item.exitCode ?? "—"}</p>
        )}
      </div>
    );
  }
  if (item.type === "historyAttachments") {
    return (
      <div className="ml-auto max-w-[80%] narrow:max-w-[95%] border border-accent-strong/40 bg-accent-border/60 px-3 py-1 text-xs text-accent-fg-faint">
        {t("chat.historyAttached", { files: attachmentLine(t, item.items) })}
      </div>
    );
  }
  if (item.type === "webActivity") {
    return (
      <div className="space-y-1">
        <WebActivityCard activity={item.activity} />
        <InjectionCheckStatus check={injectionCheck} />
      </div>
    );
  }
  if (item.type === "historyActionResult") {
    return (
      <div
        className="rounded border border-border bg-surface-sunken px-3 py-2 text-xs"
        data-testid="history-action-result"
      >
        <p className="whitespace-pre-wrap break-words font-mono">{item.text}</p>
      </div>
    );
  }
  if (item.type === "historyRejected") {
    return (
      <div className="rounded border border-warning-border/50 bg-warning-subtle/30 px-3 py-2 text-xs text-warning-fg-soft">
        <p className="font-mono">$ {item.command}</p>
        <p className="mt-1">{item.reason}</p>
      </div>
    );
  }

  return (
    <ActionCard
      item={item}
      liveOutput={liveOutput}
      waitingForInput={waitingForInput}
      injectionCheck={injectionCheck}
      onRespond={onRespond}
      onAcceptWithRule={onAcceptWithRule}
      serverId={serverId}
      sessionId={sessionId}
    />
  );
});

/** "name (Art), …" — nur Namen und Art, nie Inhalt (Spec 0110, §5). */
export function attachmentLine(
  t: (key: string) => string,
  items: { name: string; kind: ComposerAttachment["kind"] }[],
): string {
  const kindLabel = (k: ComposerAttachment["kind"]) =>
    k === "text"
      ? t("chat.attachmentKindText")
      : k === "image"
        ? t("chat.attachmentKindImage")
        : "PDF";
  return items.map((a) => `${a.name} (${kindLabel(a.kind)})`).join(", ");
}

