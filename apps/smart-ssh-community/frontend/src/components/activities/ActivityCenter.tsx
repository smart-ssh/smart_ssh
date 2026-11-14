// Issue #1325, Spec 0116: the activity icon in the header and its popover.
import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TranslateFn, TranslationKey } from "../../i18nTypes";
import { cancelActivity, dismissActivity, type Activity } from "../../api/activitiesApi";
import { useActivities } from "../../hooks/useActivities";
import { formatBytes } from "../../lib/format";
import { voiceErrorKey } from "../../settings/voiceInputSettings";
import { Button } from "../ui";

const RING_R = 9;
const RING_C = 2 * Math.PI * RING_R;

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

function Ring({ fraction, children }: { fraction: number | null; children: React.ReactNode }) {
  return (
    <span className="relative inline-flex h-6 w-6 items-center justify-center">
      <svg viewBox="0 0 24 24" className={`absolute inset-0 ${fraction === null ? "animate-spin" : ""}`} aria-hidden="true">
        <circle cx="12" cy="12" r={RING_R} fill="none" strokeWidth="2.5" className="stroke-surface-raised" />
        <circle
          cx="12"
          cy="12"
          r={RING_R}
          fill="none"
          strokeWidth="2.5"
          strokeLinecap="butt"
          className="stroke-accent"
          strokeDasharray={RING_C}
          strokeDashoffset={RING_C * (1 - (fraction ?? 0.25))}
          transform="rotate(-90 12 12)"
        />
      </svg>
      <span className="relative text-[10px] font-semibold leading-none text-fg">{children}</span>
    </span>
  );
}

function ActivityRow({ activity }: { activity: Activity }) {
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
      : null
    : a.state === "succeeded"
      ? t("activities.succeeded")
      : a.state === "cancelled"
        ? t("activities.cancelled")
        : null;

  // Speech model errors carry a code the voice texts translate (issue #1326).
  const errorText = a.errorCode?.startsWith("speech_model_")
    ? t(voiceErrorKey(a.errorCode))
    : a.errorMessage;
  return (
    <li className="border-b border-surface-raised px-3 py-2 last:border-b-0" aria-label={title}>
      <div className="flex items-start justify-between gap-2">
        <span className="min-w-0 break-words font-semibold text-fg">
          {a.state === "succeeded" && (
            <span aria-hidden="true" className="mr-1 text-success-fg">
              ✓
            </span>
          )}
          {a.state === "failed" && (
            <span aria-hidden="true" className="mr-1 text-danger-fg-soft">
              !
            </span>
          )}
          {title}
        </span>
        <span className="flex shrink-0 gap-1">
          {running && a.cancellable && (
            <Button
              size="sm"
              onClick={() => void cancelActivity(a.id).catch((e) => console.warn("activity_cancel failed:", e))}
              aria-label={t("activities.cancelNamed", { title })}
            >
              {t("activities.cancel")}
            </Button>
          )}
          {!running && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void dismissActivity(a.id).catch((e) => console.warn("activity_dismiss failed:", e))}
              aria-label={t("activities.dismissNamed", { title })}
            >
              {t("activities.dismiss")}
            </Button>
          )}
        </span>
      </div>
      {running && (
        <progress
          aria-label={title}
          aria-valuetext={sizeText ?? t("activities.inProgress")}
          value={known ? Math.min(a.done, a.total as number) : undefined}
          max={known ? (a.total as number) : undefined}
          className="mt-1.5 block h-1.5 w-full accent-[var(--color-accent)]"
        />
      )}
      {(sizeText || statusText) && (
        <p className="mt-1 text-fg-subtle">{[statusText, running ? sizeText : null].filter(Boolean).join(" · ")}</p>
      )}
      {a.state === "failed" && (
        <p className="mt-1 break-words text-danger-fg-soft">
          {t("activities.failed")}
          {errorText ? `: ${errorText}` : ""}
        </p>
      )}
    </li>
  );
}

/** Header icon with count and progress ring; hidden without activities. */
export function ActivityCenter() {
  const { t } = useTranslation();
  const list = useActivities();
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
    if (list.length === 0) setOpen(false);
  }, [list.length]);

  if (list.length === 0) return null;

  const running = list.filter((a) => a.state === "running");
  const failed = list.some((a) => a.state === "failed");
  const fraction = combinedProgress(list);
  const label =
    running.length > 0
      ? t("activities.buttonRunning", { running: running.length })
      : failed
        ? t("activities.buttonFailed")
        : t("activities.buttonDone");

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
        className="h-8 min-w-8 px-1 text-fg-muted hover:bg-surface-raised"
      >
        {running.length > 0 ? (
          <Ring fraction={fraction}>{running.length}</Ring>
        ) : failed ? (
          <span aria-hidden="true" className="text-sm font-bold text-danger-fg-soft">
            !
          </span>
        ) : (
          <span aria-hidden="true" className="text-sm font-bold text-success-fg">
            ✓
          </span>
        )}
      </Button>
      {open && (
        <dialog
          open
          id={panelId}
          aria-label={t("activities.title")}
          className="absolute left-auto right-0 top-full z-40 m-0 mt-1 p-0 text-fg max-h-[70vh] w-[min(22rem,calc(100vw-1rem))] overflow-y-auto border border-border bg-surface text-left text-xs shadow-lg"
        >
          <h2 className="border-b border-surface-raised px-3 py-2 font-heading text-xs font-semibold tracking-wide text-fg">
            {t("activities.title")}
          </h2>
          <ul>
            {list.map((a) => (
              <ActivityRow key={a.id} activity={a} />
            ))}
          </ul>
        </dialog>
      )}
    </div>
  );
}
