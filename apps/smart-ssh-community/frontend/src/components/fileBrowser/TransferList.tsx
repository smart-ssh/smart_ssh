import type { ReactNode } from "react";
import type { TranslationKey } from "../../i18nTypes";
import { useTranslation } from "react-i18next";
import { sftpCancelTransfer } from "../../api/api";
import { formatBytes } from "../../lib/format";
import { formatDuration, formatSpeed } from "../../lib/transferRate";
import { Button, Icon } from "../ui";
import type { Transfer } from "./shared";
import { StatusSquare, type StatusTone } from "../ui/StatusSquare";

interface TransferListProps {
  transfers: Transfer[];
  sessionId: string;
  /** Removes a finished transfer row. */
  onDismiss: (id: string) => void;
}

/** Full-width 6 px bar; `max` of 0 / unknown totals render no bar at all. */
function ProgressBar({ label, value, max }: { label: string; value: number; max: number }) {
  const now = Math.min(value, max);
  return (
    <div
      // eslint-disable-next-line jsx-a11y/prefer-tag-over-role -- full-width 6 px bar of the design; a native <progress> cannot be styled to it in all webviews
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={now}
      className="h-1.5 w-full bg-surface-raised"
    >
      <div className="h-full bg-accent" style={{ width: `${(now / max) * 100}%` }} />
    </div>
  );
}

/** One transfer block: status square, text, trailing icon button, optional body. */
function Block({
  tone,
  pulse,
  title,
  titleClass,
  action,
  children,
}: {
  tone: StatusTone;
  pulse?: boolean;
  title: ReactNode;
  titleClass: string;
  action: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="space-y-1 px-3 py-1.5 text-xs">
      <div className="flex items-center gap-2">
        <StatusSquare tone={tone} pulse={pulse} />
        <span className={`min-w-0 flex-1 truncate ${titleClass}`}>{title}</span>
        {action}
      </div>
      {children}
    </div>
  );
}

/** Running and finished transfers with progress, cancel and result display. */
export function TransferList({ transfers, sessionId, onDismiss }: TransferListProps) {
  const { t } = useTranslation();
  const dismissButton = (id: string) => (
    <Button
      size="sm"
      variant="ghost"
      iconOnly
      onClick={() => onDismiss(id)}
      aria-label={t("common.close")}
      title={t("common.close")}
    >
      <Icon name="x" />
    </Button>
  );
  return (
    <div className="divide-y divide-border border-b border-border bg-surface-sunken/60">
      {transfers.map((transfer) => {
        if (transfer.doneElevationEnded) {
          return (
            <Block
              key={transfer.id}
              tone="success"
              titleClass="text-fg-muted"
              title={
                <>
                  {t("fileBrowser.transferDone", { name: transfer.fileName })}{" "}
                  <span className="text-fg-subtle">{t("fileBrowser.transferElevationEnded")}</span>
                </>
              }
              action={dismissButton(transfer.id)}
            />
          );
        }
        if (transfer.stopped) {
          return (
            <Block
              key={transfer.id}
              tone="warning"
              titleClass="text-fg-muted"
              title={
                <>
                  {t(
                    transfer.stopped === "cancelled"
                      ? "fileBrowser.transferCancelled"
                      : "fileBrowser.transferStopped",
                    { name: transfer.fileName },
                  )}
                  {transfer.entriesDone !== undefined && (
                    <span className="text-fg-subtle">
                      {" — "}
                      {t("fileBrowser.entriesChanged", { count: transfer.entriesDone })}
                    </span>
                  )}
                  {transfer.incompleteRemotePath && (
                    <span className="text-warning-fg-soft">
                      {" "}
                      {t("fileBrowser.transferIncomplete", { path: transfer.incompleteRemotePath })}
                    </span>
                  )}
                </>
              }
              action={dismissButton(transfer.id)}
            />
          );
        }
        if (transfer.error) {
          return (
            <Block
              key={transfer.id}
              tone="danger"
              titleClass="text-danger-fg-soft"
              title={
                <>
                  {transfer.fileName}: {transfer.error}
                  {transfer.entriesDone !== undefined &&
                    ` — ${t("fileBrowser.entriesChanged", { count: transfer.entriesDone })}`}
                </>
              }
              action={dismissButton(transfer.id)}
            />
          );
        }
        const countKind =
          transfer.kind === "delete" || transfer.kind === "chmod" || transfer.kind === "chown";
        const bar = countKind
          ? transfer.entries?.total != null && transfer.entries.total > 0
            ? { value: transfer.entries.done, max: transfer.entries.total }
            : null
          : transfer.totalBytes !== null && transfer.totalBytes > 0
            ? { value: transfer.bytesDone ?? 0, max: transfer.totalBytes }
            : null;
        return (
          <Block
            key={transfer.id}
            tone="accent"
            pulse
            titleClass="text-fg-muted"
            title={
              <>
                {t(
                  ({
                    upload: "fileBrowser.transferUpload",
                    download: "fileBrowser.transferDownload",
                    delete: "fileBrowser.transferDelete",
                    chmod: "fileBrowser.transferChmod",
                    chown: "fileBrowser.transferChown",
                  } satisfies Record<typeof transfer.kind, TranslationKey>)[transfer.kind],
                  { name: transfer.fileName },
                )}
                {transfer.totalBytes !== null && ` (${formatBytes(transfer.totalBytes)})`}
                {transfer.folder &&
                  ` — ${t("fileBrowser.transferFiles", {
                    done: transfer.folder.filesDone,
                    total: transfer.folder.filesTotal,
                  })}`}
              </>
            }
            action={
              <Button
                size="sm"
                variant="ghost"
                iconOnly
                onClick={() =>
                  sftpCancelTransfer(sessionId, transfer.id).catch((err) =>
                    console.warn("Konnte Transfer nicht abbrechen:", err),
                  )
                }
                aria-label={t("fileBrowser.transferCancel")}
                title={t("fileBrowser.transferCancel")}
              >
                <Icon name="x" />
              </Button>
            }
          >
            {bar && <ProgressBar label={transfer.fileName} value={bar.value} max={bar.max} />}
            <div className="flex flex-wrap justify-between gap-x-3 font-mono tabular-nums text-fg-subtle">
              {countKind ? (
                <span>
                  {transfer.entries?.total != null
                    ? t("fileBrowser.entriesProgress", {
                        done: transfer.entries.done,
                        total: transfer.entries.total,
                      })
                    : (transfer.entries?.done ?? 0)}
                </span>
              ) : (
                <>
                  <span>
                    {formatBytes(transfer.bytesDone ?? 0)}
                    {transfer.totalBytes !== null && ` / ${formatBytes(transfer.totalBytes)}`}
                    {" · "}
                    <span>{formatSpeed(transfer.rate?.speed ?? null)}</span>
                  </span>
                  <span>
                    {transfer.rate?.etaSeconds != null
                      ? t("fileBrowser.transferRemaining", { time: formatDuration(transfer.rate.etaSeconds) })
                      : formatDuration(null)}
                  </span>
                </>
              )}
            </div>
          </Block>
        );
      })}
    </div>
  );
}
