import { type ChatItem } from "./chatItem";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { translateErrorCode } from "../../errorCodes";
import { Button } from "../ui";
import type { TranslateFn } from "../../i18nTypes";

// Exportiert für Komponententests (Spec 0029, Abschnitt 4: strukturelle
// DOM-Prüfung der Badge-Position) — sonst rein intern von `ChatPanel`
// genutzt.
/** Issue #600: "Details" (collapsed, copyable) and "Try again" below the
 * translated text of an AI error card. */
/** Issue #694: timeout errors name the provider and the limit. */
export function errorCardText(
  t: TranslateFn,
  item: Extract<ChatItem, { type: "error" }>,
): string {
  const d = item.details;
  if (d?.providerName && d.timeoutSecs) {
    if (item.code === "AI_TIMEOUT") {
      return t("errors.AI_TIMEOUT_NAMED", { provider: d.providerName, seconds: d.timeoutSecs });
    }
    if (item.code === "AI_TOTAL_DEADLINE") {
      return t("errors.AI_TOTAL_DEADLINE", {
        provider: d.providerName,
        minutes: Math.round(d.timeoutSecs / 60),
      });
    }
  }
  return translateErrorCode(t, item.code, item.message, { date: d?.accessReturns });
}

export function ErrorCardExtras({
  item,
  onRetryTurn,
  retryDisabled,
}: {
  item: Extract<ChatItem, { type: "error" }>;
  onRetryTurn?: (itemId: string) => void;
  retryDisabled: boolean;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState<"ok" | "failed" | null>(null);
  const d = item.details;
  const lines: string[] = [];
  if (item.code) lines.push(`code: ${item.code}`);
  if (d?.providerErrorType) lines.push(`type: ${d.providerErrorType}`);
  if (item.code) lines.push(`status: ${d?.httpStatus ?? "stream"}`);
  if (d?.providerMessage) lines.push(`message: ${d.providerMessage}`);
  const text = lines.join("\n");
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied("ok");
    } catch {
      setCopied("failed");
    }
  };
  return (
    <div className="mt-2 space-y-2 text-xs">
      {lines.length > 0 && (
        <details data-testid="chat-error-details">
          <summary className="cursor-pointer select-none text-danger-fg-softer">{t("chat.errorDetails")}</summary>
          <pre className="mt-1 whitespace-pre-wrap break-words rounded bg-danger-subtle/60 p-2 text-danger-fg-softer">{text}</pre>
          <Button size="sm" onClick={copy}>
            {t("about.copyButton")}
          </Button>
          {copied === "ok" && <span className="ml-2">{t("about.copied")}</span>}
          {copied === "failed" && <span className="ml-2">{t("about.copyFailed")}</span>}
        </details>
      )}
      {item.retryable && onRetryTurn && (
        <Button size="sm" disabled={retryDisabled} onClick={() => onRetryTurn(item.id)}>
          {t("chat.tryAgain")}
        </Button>
      )}
    </div>
  );
}

