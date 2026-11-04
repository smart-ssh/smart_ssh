/** Issue #601: helpers for the chat's context-size indicator. */

/** Share of the window from which the indicator is coloured (matches the
 * backend compaction trigger). */
export const CONTEXT_WARN_RATIO = 0.75;

/** "120k" style; below 1000 the plain number. */
export function formatTokensK(tokens: number): string {
  if (tokens < 1000) return String(tokens);
  const k = tokens / 1000;
  return `${k >= 100 ? Math.round(k) : Math.round(k * 10) / 10}k`;
}

export function isContextNearLimit(inputTokens: number, windowTokens: number): boolean {
  return windowTokens > 0 && inputTokens / windowTokens >= CONTEXT_WARN_RATIO;
}
