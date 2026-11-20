import { useTranslation } from "react-i18next";
import { CONTEXT_SEGMENTS, contextSegments, formatTokensK, isContextNearLimit } from "../lib/contextUsage";

/** Issue #601 / #1372: last real input size vs. the model window, as an
 * 8-segment meter plus numbers (shown in the chat header). */
export function ContextUsageIndicator({
  inputTokens,
  windowTokens,
}: {
  inputTokens: number;
  windowTokens: number;
}) {
  const { t } = useTranslation();
  const near = isContextNearLimit(inputTokens, windowTokens);
  const filled = contextSegments(inputTokens, windowTokens);
  return (
    <div
      data-testid="context-usage"
      data-near-limit={near ? "true" : "false"}
      title={t("chat.contextUsageTooltip")}
      className={`flex items-center gap-2 text-xs ${near ? "text-warning-fg-soft" : "text-fg-subtle"}`}
    >
      <span aria-hidden="true" data-testid="context-segments" data-filled={filled} className="flex gap-0.5">
        {Array.from({ length: CONTEXT_SEGMENTS }, (_, i) => (
          <span
            key={i}
            data-filled={i < filled ? "true" : "false"}
            className={`h-3 w-1.5 ${i < filled ? (near ? "bg-warning-fg-soft" : "bg-accent-strong") : "bg-border-strong"}`}
          />
        ))}
      </span>
      <span className="font-mono">
        {t("chat.contextUsage", {
          used: formatTokensK(inputTokens),
          window: formatTokensK(windowTokens),
        })}
      </span>
    </div>
  );
}
