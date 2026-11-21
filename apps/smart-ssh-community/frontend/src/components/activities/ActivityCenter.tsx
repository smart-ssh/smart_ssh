// Issue #1325, Spec 0116: the activity icon in the header and its popover.
import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TranslateFn, TranslationKey } from "../../i18nTypes";
import { cancelActivity, dismissActivity, type Activity } from "../../api/activitiesApi";
import { useActivities } from "../../hooks/useActivities";
import {
  discardInterrupted,
  resumeInterrupted,
  useInterruptedActivities,
} from "../../hooks/useInterruptedActivities";
import type { InterruptedRow } from "../../api/activitiesApi";
import { formatBytes } from "../../lib/format";
import { formatDuration, formatSpeed, nextRate, RATE_WINDOW_MS, type RateState } from "../../lib/transferRate";
import { voiceErrorKey } from "../../settings/voiceInputSettings";
import { Button, Icon } from "../ui";
import { kindIcon } from "./activityKind";

/** Combined progress of the running activities that know their total; `null`
 * when none does (indeterminate). */
export function combinedProgress(list: Activity[]): number | null {
  let done = 0;
  let total = 0;
  for (const a of list) {
    if (a.state !== "running" || a.total === null || a.total <= 0) continue;
    done += Math.min(a.done, a.total);
    total += a.total;
  }
  return total > 0 ? done / total : null;
}

/** Speed and remaining time per running activity (Spec 0054 moving average).
 * Lives in the trigger, which is mounted while the panel is closed, so the
 * window is filled from the progress updates before the panel opens. */
interface RateEntry {
  rate: RateState;
  done: number;
  /** Time of the last progress update (ms). */
  at: number;
  /** No update within `RATE_WINDOW_MS`: speed and remaining time are unknown. */
  stalled: boolean;
}

function useRates(list: Activity[]): Map<string, RateState> {
  const [entries, setEntries] = useState<Map<string, RateEntry>>(() => new Map());

  // Fold real progress updates (a changed `done`) and drop activities that left the running state.
  useEffect(() => {
    setEntries((prev) => {
      const now = Date.now();
      const next = new Map<string, RateEntry>();
      let changed = false;
      for (const a of list) {
        if (a.state !== "running") continue;
        const old = prev.get(a.id);
        if (old && old.done === a.done) {
          next.set(a.id, old);
        } else {
          next.set(a.id, {
            rate: nextRate(old?.rate, now, a.done, a.total),
            done: a.done,
            at: now,
            stalled: false,
          });
          changed = true;
        }
      }
      return changed || next.size !== prev.size ? next : prev;
    });
  }, [list]);

  // Without progress events nothing re-renders: a timer marks entries as stalled.
  useEffect(() => {
    let soonest: number | null = null;
    for (const e of entries.values()) {
      if (!e.stalled && (soonest === null || e.at < soonest)) soonest = e.at;
    }
    if (soonest === null) return;
    const timer = setTimeout(
      () => {
        const now = Date.now();
        setEntries((prev) => {
          let changed = false;
          const next = new Map(prev);
          for (const [id, e] of prev) {
            if (!e.stalled && now - e.at >= RATE_WINDOW_MS) {
              next.set(id, { ...e, stalled: true });
              changed = true;
            }
          }
          return changed ? next : prev;
        });
      },
      Math.max(0, soonest + RATE_WINDOW_MS - Date.now()),
    );
    return () => clearTimeout(timer);
  }, [entries]);

  return new Map(
    [...entries].map(([id, e]) => [
      id,
      e.stalled ? { ...e.rate, speed: null, etaSeconds: null } : e.rate,
    ]),
  );
}

function ProgressBar({
  label,
  valueText,
  done,
  total,
  tone,
}: {
  label: string;
  valueText: string;
  done: number;
  total: number | null;
  tone: "accent" | "warning";
}) {
  const known = total !== null && total > 0;
  const color = tone === "warning" ? "warning" : "accent";
  // Flat 6 px bar; unknown total: striped (the native indeterminate look is hidden).
  return (
    <>
      <progress
        aria-label={label}
        aria-valuetext={valueText}
        value={known ? Math.min(done, total as number) : undefined}
        max={known ? (total as number) : undefined}
        data-indeterminate={known ? undefined : "true"}
        data-testid="activity-progress-bar"
        className={
          known
            ? `mt-2 block h-1.5 w-full appearance-none overflow-hidden border-0 bg-surface-raised [&::-webkit-progress-bar]:bg-transparent ${
                tone === "warning"
                  ? "[&::-moz-progress-bar]:bg-warning [&::-webkit-progress-value]:bg-warning"
                  : "[&::-moz-progress-bar]:bg-accent [&::-webkit-progress-value]:bg-accent"
              }`
            : "sr-only"
        }
      />
      {!known && (
        // Visual stripes for the unknown total; the native indeterminate look is not styleable.
        <div
          aria-hidden="true"
          data-testid="activity-progress-stripes"
          style={{
            backgroundImage: `repeating-linear-gradient(45deg, var(--${color}) 0 6px, var(--surface-raised) 6px 12px)`,
          }}
          className="mt-2 block h-1.5 w-full overflow-hidden bg-surface-raised"
        />
      )}
    </>
  );
}

function ActivityRow({ activity, rate }: { activity: Activity; rate: RateState | undefined }) {
  const { t } = useTranslation();
  // Title and status keys come from the backend or an extension at run time.
  const tx: TranslateFn = t as unknown as TranslateFn;
  const a = activity;
  const running = a.state === "running";
  const title = tx(a.titleKey as TranslationKey, { ...a.titleParams });
  const known = a.total !== null && a.total > 0;
  const sizeText = known
    ? `${formatBytes(a.done)} / ${formatBytes(a.total as number)}`
    : running && a.done > 0
      ? formatBytes(a.done)
      : null;
  const statusText = running
    ? a.statusKey
      ? tx(a.statusKey as TranslationKey)
      : t("activities.inProgress")
    : a.state === "succeeded"
      ? t("activities.succeeded")
      : a.state === "cancelled"
        ? t("activities.cancelled")
        : t("activities.failed");
  const statusTone =
    a.state === "failed"
      ? "text-danger-fg-soft"
      : a.state === "succeeded"
        ? "text-success-fg"
        : running
          ? "text-accent-fg"
          : "text-fg-subtle";
  const metaParts = running
    ? [
        sizeText,
        known && rate?.speed != null ? formatSpeed(rate.speed) : null,
        known && rate?.etaSeconds != null
          ? t("activities.remaining", { time: formatDuration(rate.etaSeconds) })
          : null,
      ].filter(Boolean)
    : [];

  // Speech model errors carry a code the voice texts translate (issue #1326).
  const errorText = a.errorCode?.startsWith("speech_model_")
    ? t(voiceErrorKey(a.errorCode))
    : a.errorMessage;
  return (
    <li className="border-b border-border px-4 py-3 last:border-b-0" aria-label={title}>
      <div className="flex items-start gap-2">
        {a.state === "succeeded" ? (
          <Icon name="check" className="mt-0.5 shrink-0 text-success-fg" />
        ) : a.state === "failed" ? (
          <Icon name="warn" className="mt-0.5 shrink-0 text-danger-fg-soft" />
        ) : (
          <Icon name={kindIcon(a.kind)} className="mt-0.5 shrink-0 text-fg-muted" />
        )}
        <span className="min-w-0 flex-1 break-words text-sm font-bold text-fg">{title}</span>
        <span className={`label-condensed shrink-0 uppercase ${statusTone}`}>{statusText}</span>
      </div>
      {running && (
        <ProgressBar
          label={title}
          valueText={sizeText ?? t("activities.inProgress")}
          done={a.done}
          total={a.total}
          tone="accent"
        />
      )}
      {metaParts.length > 0 && (
        <p className="mt-1.5 font-mono text-[11px] text-fg-subtle">{metaParts.join(" · ")}</p>
      )}
      {a.state === "failed" && errorText && (
        <p className="mt-1.5 break-words text-danger-fg-soft">{errorText}</p>
      )}
      {((running && a.cancellable) || !running) && (
        <div className="mt-2 flex gap-1">
          {running && a.cancellable && (
            <Button
              size="sm"
              iconOnly
              variant="ghost"
              onClick={() => void cancelActivity(a.id).catch((e) => console.warn("activity_cancel failed:", e))}
              aria-label={t("activities.cancelNamed", { title })}
              title={t("activities.cancel")}
            >
              <Icon name="x" />
            </Button>
          )}
          {!running && (
            <Button
              size="sm"
              iconOnly
              variant="ghost"
              onClick={() => void dismissActivity(a.id).catch((e) => console.warn("activity_dismiss failed:", e))}
              aria-label={t("activities.dismissNamed", { title })}
              title={t("activities.dismiss")}
            >
              <Icon name="x" />
            </Button>
          )}
        </div>
      )}
    </li>
  );
}

/** An activity a quit interrupted: offers Resume and Discard (issue #1327). */
function InterruptedRowItem({ row }: { row: InterruptedRow }) {
  const { t } = useTranslation();
  const tx: TranslateFn = t as unknown as TranslateFn;
  const [failed, setFailed] = useState(false);
  const title = tx(row.titleKey as TranslationKey, { ...row.titleParams });
  const known = row.total !== null && row.total > 0;
  const sizeText = known ? `${formatBytes(row.done)} / ${formatBytes(row.total as number)}` : null;
  return (
    <li className="border-b border-border bg-warning-subtle px-4 py-3 last:border-b-0" aria-label={title}>
      <div className="flex items-start gap-2">
        <Icon name={kindIcon(row.kind)} className="mt-0.5 shrink-0 text-fg-muted" />
        <span className="min-w-0 flex-1 break-words text-sm font-bold text-fg">{title}</span>
        <span className="label-condensed shrink-0 uppercase text-warning-fg">{t("activities.interrupted")}</span>
      </div>
      {known && (
        <ProgressBar
          label={title}
          valueText={sizeText ?? t("activities.interrupted")}
          done={row.done}
          total={row.total}
          tone="warning"
        />
      )}
      {sizeText && <p className="mt-1.5 font-mono text-[11px] text-fg-subtle">{sizeText}</p>}
      <div className="mt-2 flex gap-1">
        <Button
          size="sm"
          onClick={() => {
            setFailed(false);
            void resumeInterrupted(row).catch((e) => {
              console.warn("resume failed:", e);
              setFailed(true);
            });
          }}
          aria-label={t("activities.resumeNamed", { title })}
        >
          {t("activities.resume")}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void discardInterrupted(row).catch((e) => console.warn("discard failed:", e))}
          aria-label={t("activities.discardNamed", { title })}
        >
          {t("activities.discard")}
        </Button>
      </div>
      {failed && <p className="mt-1.5 break-words text-danger-fg-soft">{t("activities.resumeFailed")}</p>}
    </li>
  );
}

/** Header trigger (activity icon, mono count, thin progress rule); hidden without activities. */
export function ActivityCenter() {
  const { t } = useTranslation();
  const list = useActivities();
  const interrupted = useInterruptedActivities();
  const rates = useRates(list);
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  const listRef = useRef(list);
  listRef.current = list;

  const close = (returnFocus: boolean) => {
    setOpen(false);
    // A success that was seen is cleared; a failure stays until dismissed.
    for (const a of listRef.current) {
      if (a.state === "succeeded") void dismissActivity(a.id).catch(() => undefined);
    }
    if (returnFocus) buttonRef.current?.focus();
  };
  const closeRef = useRef(close);
  closeRef.current = close;

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeRef.current(true);
    };
    const onPointer = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) closeRef.current(false);
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onPointer);
    };
  }, [open]);

  // Nothing left to show: the popover goes with the last row.
  useEffect(() => {
    if (list.length === 0 && interrupted.length === 0) setOpen(false);
  }, [list.length, interrupted.length]);

  if (list.length === 0 && interrupted.length === 0) return null;

  const running = list.filter((a) => a.state === "running");
  const failedCount = list.filter((a) => a.state === "failed").length;
  const failed = failedCount > 0;
  const fraction = combinedProgress(list);
  const label =
    running.length > 0
      ? t("activities.buttonRunning", { running: running.length })
      : failed
        ? t("activities.buttonFailed")
        : interrupted.length > 0
          ? t("activities.buttonInterrupted")
          : t("activities.buttonDone");
  const summary = [
    running.length > 0 ? t("activities.summaryRunning", { count: running.length }) : null,
    interrupted.length > 0 ? t("activities.summaryInterrupted", { count: interrupted.length }) : null,
    failedCount > 0 ? t("activities.summaryFailed", { count: failedCount }) : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div ref={wrapRef} className="relative">
      <Button
        ref={buttonRef}
        variant="bare"
        aria-label={label}
        aria-expanded={open}
        aria-controls={open ? panelId : undefined}
        aria-haspopup="dialog"
        data-testid="activity-center-button"
        onClick={() => (open ? close(false) : setOpen(true))}
        className={`relative inline-flex h-8 min-w-8 items-center justify-center gap-1 px-1.5 text-fg-muted hover:bg-surface-raised ${
          open ? "border-t-2 border-accent" : ""
        }`}
      >
        {running.length > 0 ? (
          <>
            <Icon name="activity" className="text-accent" />
            <span className="font-mono text-xs font-semibold text-accent-fg">{running.length}</span>
          </>
        ) : failed || interrupted.length > 0 ? (
          <Icon name="warn" className="text-danger-fg-soft" />
        ) : (
          <Icon name="check" className="text-success-fg" />
        )}
        {running.length > 0 && (
          <span
            aria-hidden="true"
            data-testid="activity-progress-rule"
            data-indeterminate={fraction === null ? "true" : undefined}
            className="absolute inset-x-0 bottom-0 h-0.5 bg-surface-raised"
          >
            <span
              className={`block h-full bg-accent ${fraction === null ? "w-full animate-pulse" : ""}`}
              style={fraction === null ? undefined : { width: `${Math.round(fraction * 100)}%` }}
            />
          </span>
        )}
      </Button>
      {open && (
        <dialog
          open
          id={panelId}
          aria-label={t("activities.title")}
          className="absolute left-auto right-0 top-full z-40 m-0 mt-1 max-h-[70vh] w-[min(27.5rem,calc(100vw-1rem))] overflow-y-auto border border-border-strong bg-surface-raised p-0 text-left text-xs text-fg shadow-offset"
        >
          <header className="flex items-baseline justify-between gap-3 border-b border-border-strong px-4 py-2.5">
            <h2 className="font-heading text-sm font-semibold uppercase tracking-wide text-fg">
              {t("activities.title")}
            </h2>
            {summary && <span className="font-mono text-[11px] text-fg-subtle">{summary}</span>}
          </header>
          <ul>
            {interrupted.map((r) => (
              <InterruptedRowItem key={r.id} row={r} />
            ))}
            {list.map((a) => (
              <ActivityRow key={a.id} activity={a} rate={rates.get(a.id)} />
            ))}
          </ul>
        </dialog>
      )}
    </div>
  );
}
