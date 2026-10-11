import { useLayoutEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { LiveOutput } from "../liveOutput";

/** Distance (px) from the bottom that still counts as "at the end". */
const STICK_THRESHOLD_PX = 16;

/** Issue #325 (Spec 0106): live output of the running command inside its
 * chat block — stdout and stderr in the same colours as the final result.
 * Follows new output while the user has not scrolled up inside the block;
 * scrolling back to the end resumes following. */
export function LiveCommandOutput({ output }: { output: LiveOutput }) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const followRef = useRef(true);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && followRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [output.stdout, output.stderr, output.truncated]);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    followRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= STICK_THRESHOLD_PX;
  };

  return (
    <div
      ref={scrollRef}
      onScroll={handleScroll}
      role="log"
      aria-live="off"
      aria-label={t("actionCard.liveOutputLabel")}
      data-testid="live-command-output"
      className="mt-2 max-h-64 space-y-1 overflow-y-auto rounded bg-slate-950 p-2 font-mono text-xs"
    >
      {output.stdout && <pre className="whitespace-pre-wrap text-slate-300">{output.stdout}</pre>}
      {output.stderr && <pre className="whitespace-pre-wrap text-red-300">{output.stderr}</pre>}
      {output.truncated && (
        <p className="font-sans text-amber-300">{t("actionCard.liveOutputTruncatedNotice")}</p>
      )}
    </div>
  );
}
