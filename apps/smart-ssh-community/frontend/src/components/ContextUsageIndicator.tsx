import { useTranslation } from "react-i18next";
import { formatTokensK, isContextNearLimit } from "../contextUsage";

/** Issue #601: last real input size vs. the model window. Numbers only. */
export function ContextUsageIndicator({
  inputTokens,
  windowTokens,
}: {
  inputTokens: number;
  windowTokens: number;
}) {
  const { t } = useTranslation();
  const near = isContextNearLimit(inputTokens, windowTokens);
  return (
    <div
      data-testid="context-usage"
      data-near-limit={near ? "true" : "false"}
      title={t("chat.contextUsageTooltip")}
      className={`px-3 pt-2 text-xs ${near ? "text-amber-300" : "text-slate-400"}`}
    >
      {t("chat.contextUsage", {
        used: formatTokensK(inputTokens),
        window: formatTokensK(windowTokens),
      })}
    </div>
  );
}
